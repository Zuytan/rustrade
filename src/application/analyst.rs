use crate::application::candle_aggregator::CandleAggregator;
use crate::application::strategies::{AnalysisContext, TradingStrategy};
use crate::application::trailing_stops::StopState;
use crate::domain::ports::ExecutionService;
use crate::domain::types::{MarketEvent, OrderSide, TradeProposal};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use ta::indicators::{
    AverageTrueRange, BollingerBands, MovingAverageConvergenceDivergence, RelativeStrengthIndex,
    SimpleMovingAverage,
};
use ta::Next;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;

struct SymbolState {
    fast_sma: SimpleMovingAverage,
    slow_sma: SimpleMovingAverage,
    trend_sma: SimpleMovingAverage,
    rsi: RelativeStrengthIndex,
    macd: MovingAverageConvergenceDivergence,
    last_fast_sma: Option<f64>,
    last_slow_sma: Option<f64>,
    last_trend_sma: Option<f64>,
    last_rsi: Option<f64>,
    last_macd_value: Option<f64>,
    last_macd_signal: Option<f64>,
    last_macd_histogram: Option<f64>,
    last_was_above: Option<bool>,
    last_signal_time: i64,
    atr: AverageTrueRange,
    last_atr: Option<f64>,
    bb: BollingerBands,
    last_bb_lower: Option<f64>,
    last_bb_upper: Option<f64>,
    last_bb_middle: Option<f64>,
    trailing_stop: StopState, // State machine replacing entry_price/peak_price/trailing_stop_price
    pending_order: Option<OrderSide>, // Track in-flight orders
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalystConfig {
    pub fast_sma_period: usize,
    pub slow_sma_period: usize,
    pub max_positions: usize,
    pub trade_quantity: Decimal,
    pub sma_threshold: f64,
    pub order_cooldown_seconds: u64,
    pub risk_per_trade_percent: f64,
    pub strategy_mode: crate::config::StrategyMode,
    pub trend_sma_period: usize,
    pub rsi_period: usize,
    pub macd_fast_period: usize,
    pub macd_slow_period: usize,
    pub macd_signal_period: usize,
    pub trend_divergence_threshold: f64,
    pub trailing_stop_atr_multiplier: f64,
    pub atr_period: usize,
    pub rsi_threshold: f64,                // New Configurable Threshold
    pub trend_riding_exit_buffer_pct: f64, // Trend Riding Strategy
    pub mean_reversion_rsi_exit: f64,
    pub mean_reversion_bb_period: usize,
    pub slippage_pct: f64,
    pub max_position_size_pct: f64,
}

impl From<crate::config::Config> for AnalystConfig {
    fn from(config: crate::config::Config) -> Self {
        Self {
            fast_sma_period: config.fast_sma_period,
            slow_sma_period: config.slow_sma_period,
            max_positions: config.max_positions,
            trade_quantity: config.trade_quantity,
            sma_threshold: config.sma_threshold,
            order_cooldown_seconds: config.order_cooldown_seconds,
            risk_per_trade_percent: config.risk_per_trade_percent,
            strategy_mode: config.strategy_mode,
            trend_sma_period: config.trend_sma_period,
            rsi_period: config.rsi_period,
            macd_fast_period: config.macd_fast_period,
            macd_slow_period: config.macd_slow_period,
            macd_signal_period: config.macd_signal_period,
            trend_divergence_threshold: config.trend_divergence_threshold,
            rsi_threshold: config.rsi_threshold,
            trailing_stop_atr_multiplier: config.trailing_stop_atr_multiplier,
            atr_period: config.atr_period,
            trend_riding_exit_buffer_pct: config.trend_riding_exit_buffer_pct,
            mean_reversion_rsi_exit: config.mean_reversion_rsi_exit,
            mean_reversion_bb_period: config.mean_reversion_bb_period,
            slippage_pct: config.slippage_pct,
            max_position_size_pct: config.max_position_size_pct,
        }
    }
}

pub struct Analyst {
    market_rx: Receiver<MarketEvent>,
    proposal_tx: Sender<TradeProposal>,
    execution_service: Arc<dyn ExecutionService>,
    strategy: Arc<dyn TradingStrategy>,
    config: AnalystConfig,
    // Per-symbol states
    symbol_states: HashMap<String, SymbolState>,
    candle_aggregator: CandleAggregator,
}

impl Analyst {
    pub fn new(
        market_rx: Receiver<MarketEvent>,
        proposal_tx: Sender<TradeProposal>,
        execution_service: Arc<dyn ExecutionService>,
        strategy: Arc<dyn TradingStrategy>,
        config: AnalystConfig,
        repository: Option<Arc<crate::infrastructure::persistence::repositories::CandleRepository>>,
    ) -> Self {
        Self {
            market_rx,
            proposal_tx,
            execution_service,
            strategy,
            config,
            symbol_states: HashMap::new(),
            candle_aggregator: CandleAggregator::new(repository),
        }
    }

    pub async fn run(&mut self) {
        info!(
            "Analyst started (Multi-Symbol Dual SMA). Cache size: {}",
            self.config.max_positions
        );

        while let Some(event) = self.market_rx.recv().await {
            match event {
                MarketEvent::Quote {
                    symbol,
                    price,
                    timestamp,
                } => {
                    if let Some(candle) = self.candle_aggregator.on_quote(&symbol, price, timestamp)
                    {
                        self.process_candle(candle).await;
                    }
                }
                MarketEvent::Candle(candle) => {
                    self.process_candle(candle).await;
                }
            }
        }
    }

    async fn process_candle(&mut self, candle: crate::domain::types::Candle) {
        let symbol = candle.symbol;
        let price = candle.close;
        let timestamp = candle.timestamp * 1000; // Convert seconds to millis for compatibility with existing logic

        let price_f64 = price.to_f64().unwrap_or(0.0);

        // Get or initialize state for this symbol
        let fast_period = self.config.fast_sma_period;
        let slow_period = self.config.slow_sma_period;
        let trend_period = self.config.trend_sma_period;
        let rsi_period = self.config.rsi_period;

        let state = self
            .symbol_states
            .entry(symbol.clone())
            .or_insert_with(|| SymbolState {
                fast_sma: SimpleMovingAverage::new(fast_period).unwrap(),
                slow_sma: SimpleMovingAverage::new(slow_period).unwrap(),
                trend_sma: SimpleMovingAverage::new(trend_period).unwrap(),
                rsi: RelativeStrengthIndex::new(rsi_period).unwrap(),
                macd: MovingAverageConvergenceDivergence::new(
                    self.config.macd_fast_period,
                    self.config.macd_slow_period,
                    self.config.macd_signal_period,
                )
                .unwrap(),
                last_fast_sma: None,
                last_slow_sma: None,
                last_trend_sma: None,
                last_rsi: None,
                last_macd_value: None,
                last_macd_signal: None,
                last_macd_histogram: None,
                last_was_above: None,
                last_signal_time: 0,
                atr: AverageTrueRange::new(self.config.atr_period).unwrap(),
                last_atr: None,
                bb: BollingerBands::new(self.config.mean_reversion_bb_period, 2.0).unwrap(),
                last_bb_lower: None,
                last_bb_upper: None,
                last_bb_middle: None,
                trailing_stop: StopState::NoPosition,
                pending_order: None,
            });

        let current_fast = state.fast_sma.next(price_f64);
        let current_slow = state.slow_sma.next(price_f64);
        let current_trend = state.trend_sma.next(price_f64);
        let current_rsi = state.rsi.next(price_f64);
        let current_macd = state.macd.next(price_f64);
        let current_atr = state.atr.next(price_f64);
        let current_bb = state.bb.next(price_f64);

        // --- Trailing Stop Check (Priority Exit) ---
        // --- Pending Order Synchronization ---
        // Check if pending orders are confirmed by portfolio update
        // CRITICAL FIX: Check portfolio state BEFORE blocking signals
        if let Ok(portfolio) = self.execution_service.get_portfolio().await {
            let has_position = portfolio
                .positions
                .get(&symbol)
                .map(|p| p.quantity > Decimal::ZERO)
                .unwrap_or(false);

            if let Some(pending) = state.pending_order {
                match pending {
                    OrderSide::Buy => {
                        if has_position {
                            info!(
                                "Analyst: Pending Buy for {} CONFIRMED. Clearing pending state.",
                                symbol
                            );
                            state.pending_order = None;
                            // StopState will be activated by strategy signal
                        }
                    }
                    OrderSide::Sell => {
                        if !has_position {
                            info!(
                                "Analyst: Pending Sell for {} CONFIRMED. Clearing pending state and stops.",
                                symbol
                            );
                            state.pending_order = None;
                            state.trailing_stop.on_sell();
                        }
                    }
                }
            }
        }

        // --- Check Trailing Stop (Priority Exit) ---
        let mut trailing_stop_triggered = false;
        if state.pending_order != Some(OrderSide::Sell) {
            if let Some(atr) = state.last_atr {
                if atr > 0.0 {
                    if let Some(trigger) = state.trailing_stop.on_price_update(
                        price_f64,
                        atr,
                        self.config.trailing_stop_atr_multiplier,
                    ) {
                        trailing_stop_triggered = true;
                        info!(
                            "Analyst: Trailing stop HIT for {} at {:.2} (Stop: {:.2}, Entry: {:.2})",
                            symbol, trigger.exit, trigger.stop, trigger.entry
                        );
                    }
                }
            }
        }

        // Check for Crossover if we have previous SMA data
        if let (Some(_prev_fast), Some(_prev_slow)) = (state.last_fast_sma, state.last_slow_sma) {
            // Current state relative to threshold
            let is_definitively_above =
                current_fast > current_slow * (1.0 + self.config.sma_threshold);
            let is_definitively_below =
                current_fast < current_slow * (1.0 - self.config.sma_threshold);

            let mut signal = None;

            // Use stateful was_above
            match state.last_was_above {
                None => {
                    // Warm up: initialize state on first definitive move SILENTLY
                    if is_definitively_above {
                        state.last_was_above = Some(true);
                    } else if is_definitively_below {
                        state.last_was_above = Some(false);
                    }
                }
                Some(true) => {
                    // We WERE above. Switch to false if definitively below.
                    if is_definitively_below {
                        state.last_was_above = Some(false);
                        signal = Some(OrderSide::Sell);
                    }
                }
                Some(false) => {
                    // We WERE below. Switch to true if definitively above.
                    if is_definitively_above {
                        state.last_was_above = Some(true);
                        signal = Some(OrderSide::Buy);
                    }
                }
            }

            // --- Trailing Stop Override (Priority Exit) ---
            // If trailing stop is triggered, force sell signal regardless of crossover
            if trailing_stop_triggered {
                signal = Some(OrderSide::Sell);
            } else if let Some(OrderSide::Sell) = signal {
                // If we have an active trailing stop, suppress SMA-cross sell signals
                // Only exit on the trailing stop itself
                if state.trailing_stop.is_active() {
                    info!(
                        "Analyst: SMA-cross sell signal SUPPRESSED for {} - Using trailing stop instead",
                        symbol
                    );
                    signal = None;
                }
            }

            // --- Use Injected Strategy (Strategy Pattern) ---
            let analysis_ctx = AnalysisContext {
                symbol: symbol.clone(),
                current_price: price,
                price_f64,
                fast_sma: current_fast,
                slow_sma: current_slow,
                trend_sma: current_trend,
                rsi: current_rsi,
                macd_value: current_macd.macd,
                macd_signal: current_macd.signal,
                macd_histogram: current_macd.histogram,
                last_macd_histogram: state.last_macd_histogram,
                atr: current_atr,
                bb_lower: current_bb.lower,
                bb_upper: current_bb.upper,
                bb_middle: current_bb.average,
                has_position: state.trailing_stop.is_active(),
                timestamp,
            };

            if let Some(strategy_signal) = self.strategy.analyze(&analysis_ctx) {
                info!(
                    "Analyst [{}]: {} - {}",
                    self.strategy.name(),
                    symbol,
                    strategy_signal.reason
                );
                signal = Some(strategy_signal.side);
            }

            // --- Long-Only Constraint (Prevent Short Selling) ---
            if let Some(OrderSide::Sell) = signal {
                // We must check if we actually have a position to sell
                if let Ok(portfolio) = self.execution_service.get_portfolio().await {
                    let position = portfolio.positions.get(&symbol);

                    match position {
                        None => {
                            info!(
                                "Analyst: BLOCKING Sell signal for {} - No position held (preventing short selling)",
                                symbol
                            );
                            signal = None;
                        }
                        Some(pos) if pos.quantity <= Decimal::ZERO => {
                            info!(
                                "Analyst: BLOCKING Sell signal for {} - Position quantity is zero or negative ({})",
                                symbol, pos.quantity
                            );
                            signal = None;
                        }
                        Some(pos) => {
                            info!(
                                "Analyst: ALLOWING Sell signal for {} - Position exists with quantity {}",
                                symbol, pos.quantity
                            );
                            // OK to sell - we own it
                        }
                    }
                }
            }

            if let Some(side) = signal {
                // Check pending state - only block if SAME direction
                // Allow opposite direction (e.g., Buy can override pending Sell if conditions met)
                if let Some(pending) = state.pending_order {
                    if pending == side {
                        info!(
                            "Analyst: Signal {:?} for {} BLOCKED due to pending {:?} order.",
                            side, symbol, pending
                        );
                        signal = None;
                    }
                }
            }

            if let Some(side) = signal {
                // Enforce cooldown
                let cooldown_ms = self.config.order_cooldown_seconds * 1000;
                if timestamp - state.last_signal_time >= cooldown_ms as i64 {
                    state.last_signal_time = timestamp;

                    let quantity = Self::calculate_trade_quantity(
                        &self.config,
                        &self.execution_service,
                        &symbol,
                        price,
                    )
                    .await;

                    if quantity == Decimal::ZERO {
                        info!(
                            "Analyst: Final quantity is zero for {}, skipping trade.",
                            symbol
                        );
                    } else {
                        info!(
                            "Analyst: Signal Detected {:?} for {} (Dual SMA Portfolio Strategy)",
                            side, symbol
                        );

                        let proposal = TradeProposal {
                            symbol: symbol.clone(),
                            side,
                            price,
                            quantity,
                            reason: format!(
                                "Portfolio Dual SMA Crossover: Fast {:.2} slow {:.2} | Allocated {:.2}% Equity",
                                current_fast,
                                current_slow,
                                self.config.risk_per_trade_percent * 100.0
                            ),
                            timestamp,
                        };

                        if let Err(e) = self.proposal_tx.send(proposal).await {
                            tracing::error!(
                                "Analyst: Failed to send proposal for {}: {}",
                                symbol,
                                e
                            );
                        } else {
                            // Successfully sent proposal -> Mark as pending
                            state.pending_order = Some(side);
                        }

                        // Update trailing stop state based on signal type
                        if let Some(state) = self.symbol_states.get_mut(&symbol) {
                            match side {
                                OrderSide::Buy => {
                                    // Initialize trailing stop on buy (only if ATR is valid)
                                    if let Some(atr) = state.last_atr {
                                        if atr > 0.0 && !state.trailing_stop.is_active() {
                                            state.trailing_stop = StopState::on_buy(
                                                price_f64,
                                                atr,
                                                self.config.trailing_stop_atr_multiplier,
                                            );
                                            if let Some(stop_price) =
                                                state.trailing_stop.get_stop_price()
                                            {
                                                info!(
                                                    "Analyst: Trailing stop INITIALIZED for {} - Entry: {:.2}, Stop: {:.2} (ATR: {:.2})",
                                                    symbol, price_f64, stop_price, atr
                                                );
                                            }
                                        }
                                    }
                                }
                                OrderSide::Sell => {
                                    // Clear trailing stop on sell
                                    state.trailing_stop.on_sell();
                                    info!(
                                        "Analyst: Trailing stop CLEARED for {} after exit at {:.2}",
                                        symbol, price_f64
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Update State
        if let Some(state) = self.symbol_states.get_mut(&symbol) {
            state.last_fast_sma = Some(current_fast);
            state.last_slow_sma = Some(current_slow);
            state.last_trend_sma = Some(current_trend);
            state.last_rsi = Some(current_rsi);
            state.last_macd_value = Some(current_macd.macd);
            state.last_macd_signal = Some(current_macd.signal);
            state.last_macd_histogram = Some(current_macd.histogram);
            state.last_atr = Some(current_atr);
            state.last_bb_lower = Some(current_bb.lower);
            state.last_bb_upper = Some(current_bb.upper);
            state.last_bb_middle = Some(current_bb.average);
        }
    }

    #[allow(dead_code)]
    fn apply_advanced_filters(
        signal: &mut Option<OrderSide>,
        symbol: &str,
        side: OrderSide,
        price_f64: f64,
        current_trend: f64,
        current_rsi: f64,
        macd_val: f64,
        macd_hist: f64,
        last_macd_histogram: Option<f64>,
        rsi_threshold: f64,
    ) {
        match side {
            OrderSide::Buy => {
                // Filter: Price > Trend SMA AND RSI < 55 AND MACD Histogram > 0 and Rising
                let trend_filter = price_f64 > current_trend;
                // Use configurable threshold (e.g., 70.0 for momentum)
                let rsi_filter = current_rsi < rsi_threshold;

                let prev_hist = last_macd_histogram.unwrap_or(0.0);

                let macd_filter = macd_hist > 0.0 && macd_hist > prev_hist;

                if !trend_filter || !rsi_filter || !macd_filter {
                    info!(
                        "Analyst: Advanced Buy Signal for {} REJECTED by filters (Trend: {}, RSI: {:.2} (limit {:.1}), MACD: {:.4}, Hist: {:.4}, PrevHist: {:.4})",
                        symbol,
                        trend_filter,
                        current_rsi,
                        rsi_threshold,
                        macd_val,
                        macd_hist,
                        prev_hist
                    );
                    *signal = None;
                }
            }
            OrderSide::Sell => {
                // Filter: Sell if trend breaks OR RSI overbought OR MACD turns negative
                let rsi_overbought = current_rsi > 75.0;
                let trend_break = price_f64 < current_trend;
                let macd_negative = macd_hist < 0.0;

                if rsi_overbought || trend_break || macd_negative {
                    info!(
                        "Analyst: Advanced Sell Signal for {} confirmed by RSI/Trend/MACD (RSI: {:.2}, TrendBreak: {}, MACDHist: {:.4})",
                        symbol, current_rsi, trend_break, macd_hist
                    );
                }
            }
        }
    }

    async fn calculate_trade_quantity(
        config: &AnalystConfig,
        execution_service: &Arc<dyn ExecutionService>,
        symbol: &str,
        price: Decimal,
    ) -> Decimal {
        let mut quantity = config.trade_quantity;

        // Determine if we should use risk-based sizing
        // We use risk-based sizing if risk_per_trade_percent is set
        let use_risk_size = config.risk_per_trade_percent > 0.0;

        if use_risk_size {
            let portfolio_result = execution_service.get_portfolio().await;
            if let Ok(portfolio) = portfolio_result {
                let mut total_equity = portfolio.cash;
                for pos in portfolio.positions.values() {
                    total_equity += pos.quantity * pos.average_price;
                }

                info!(
                    "Analyst: Portfolio State for {}: Cash={}, TotalEquity={}, Price={}",
                    symbol, portfolio.cash, total_equity, price
                );

                if total_equity > Decimal::ZERO && price > Decimal::ZERO {
                    // 1. Calculate the target amount to allocate based on risk_per_trade_percent
                    let mut target_amt = total_equity * Decimal::from_f64_retain(config.risk_per_trade_percent).unwrap_or(Decimal::ZERO);
                    
                    info!(
                        "Analyst: Initial target amount for {} ({}% of equity): ${}",
                        symbol,
                        config.risk_per_trade_percent * 100.0,
                        target_amt
                    );

                    // 2. Apply Caps
                    // Cap 1: Max Positions bucket (if max_positions > 0)
                    if config.max_positions > 0 {
                        let max_bucket = total_equity / Decimal::from(config.max_positions);
                        let before = target_amt;
                        target_amt = target_amt.min(max_bucket);
                        if target_amt < before {
                            info!(
                                "Analyst: Capped {} by max_positions bucket: ${} -> ${}",
                                symbol, before, target_amt
                            );
                        }
                    }

                    // Cap 2: Max Position Size % (acts as a hard cap, applied independently)
                    if config.max_position_size_pct > 0.0 {
                        let max_pos_val = total_equity * Decimal::from_f64_retain(config.max_position_size_pct).unwrap_or(Decimal::ZERO);
                        let before = target_amt;
                        target_amt = target_amt.min(max_pos_val);
                        if target_amt < before {
                            info!(
                                "Analyst: Capped {} by max_position_size_pct ({}%): ${} -> ${}",
                                symbol,
                                config.max_position_size_pct * 100.0,
                                before,
                                target_amt
                            );
                        }
                    }

                    quantity = (target_amt / price).round_dp(4);
                    
                    info!(
                        "Analyst: Final quantity for {}: {} shares (${} / ${} per share)",
                        symbol, quantity, target_amt, price
                    );
                } else {
                    info!(
                        "Analyst: Cannot calculate quantity for {} - TotalEquity={}, Price={}",
                        symbol, total_equity, price
                    );
                }
            } else {
                info!(
                    "Analyst: Failed to get portfolio for {} quantity calculation",
                    symbol
                );
            }
        } else {
            info!(
                "Analyst: Using static quantity for {}: {}",
                symbol, quantity
            );
        }
        
        quantity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::Candle;
    use std::sync::Once;
    use tokio::sync::mpsc;
    use tokio::sync::RwLock;

    static INIT: Once = Once::new();

    fn setup_logging() {
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::FmtSubscriber::builder()
                .with_max_level(tracing::Level::INFO)
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
        });
    }

    #[tokio::test]
    async fn test_golden_cross() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);
        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::config::StrategyMode::Standard,
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 55.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            max_position_size_pct: 0.0,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(market_rx, proposal_tx, exec_service, strategy, config, None);

        tokio::spawn(async move {
            analyst.run().await;
        });

        use crate::domain::types::Candle;

        // Dual SMA (2, 3)
        let prices = [100.0, 100.0, 100.0, 90.0, 110.0, 120.0];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "BTC".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        let proposal = proposal_rx.recv().await.expect("Should receive buy signal");
        assert_eq!(proposal.side, OrderSide::Buy);
        assert_eq!(proposal.quantity, Decimal::from(1));
    }

    #[tokio::test]
    async fn test_prevent_short_selling() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);
        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::config::StrategyMode::Standard,
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 55.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            max_position_size_pct: 0.1,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(market_rx, proposal_tx, exec_service, strategy, config, None);

        tokio::spawn(async move {
            analyst.run().await;
        });

        // Simulating a Death Cross without holding the asset
        // Prices: 100, 100, 100 -> SMAs aligned
        // 120 -> Fast pulls up
        // 70 -> Fast pulls down below Slow -> Death Cross
        let prices = [100.0, 100.0, 100.0, 120.0, 70.0];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "AAPL".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        let mut sell_detected = false;
        // Wait briefly to ensure we process messages
        if let Ok(Some(proposal)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), proposal_rx.recv()).await
        {
            if proposal.side == OrderSide::Sell {
                sell_detected = true;
            }
        }
        assert!(
            !sell_detected,
            "Should NOT receive sell signal on empty portfolio (Short Selling Prevented)"
        );
    }

    #[tokio::test]
    async fn test_sell_signal_with_position() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        // Pre-load position so Sell matches verify logic
        let pos = crate::domain::portfolio::Position {
            symbol: "BTC".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(100),
        };
        portfolio.positions.insert("BTC".to_string(), pos);

        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::config::StrategyMode::Standard,
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 55.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            max_position_size_pct: 0.1,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(market_rx, proposal_tx, exec_service, strategy, config);

        tokio::spawn(async move {
            analyst.run().await;
        });

        let prices = [100.0, 100.0, 100.0, 120.0, 70.0];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "BTC".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        let mut sell_detected = false;
        while let Ok(Some(proposal)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), proposal_rx.recv()).await
        {
            if proposal.side == OrderSide::Sell {
                sell_detected = true;
                break;
            }
        }
        assert!(
            sell_detected,
            "Should receive sell signal when holding position"
        );
    }

    #[tokio::test]
    async fn test_dynamic_quantity_scaling() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        // 100k account
        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        // Risk 2% (0.02)
        let config = AnalystConfig {
            fast_sma_period: 1,
            slow_sma_period: 2,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.02,
            strategy_mode: crate::config::StrategyMode::Standard,
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 55.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            max_position_size_pct: 0.1,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(market_rx, proposal_tx, exec_service, strategy, config);

        tokio::spawn(async move {
            analyst.run().await;
        });

        // P: 110, 110 -> SMAs 110
        // P: 90 -> fast 90, slow 100 (F < S)
        // P: 100 -> fast 100, slow 95 (F > S) -> Golden Cross at $100
        let prices = [110.0, 110.0, 90.0, 100.0];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "AAPL".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        let proposal = proposal_rx.recv().await.expect("Should receive buy signal");
        assert_eq!(proposal.side, OrderSide::Buy);

        // Final Price = 100
        // Equity = 100,000
        // Risk = 2% of 100,000 = 2,000
        // Qty = 2,000 / 100 = 20
        assert_eq!(proposal.quantity, Decimal::from(20));
    }

    #[tokio::test]
    async fn test_multi_symbol_isolation() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        // Give explicit ETH position so Sell works
        portfolio.positions.insert(
            "ETH".to_string(),
            crate::domain::portfolio::Position {
                symbol: "ETH".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::from(100),
            },
        );
        let portfolio_lock = Arc::new(RwLock::new(portfolio));

        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        // 2 slots
        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 2,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::config::StrategyMode::Standard,
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 55.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            max_position_size_pct: 0.1,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(market_rx, proposal_tx, exec_service, strategy, config);

        tokio::spawn(async move {
            analyst.run().await;
        });

        // Interleave BTC and ETH
        // BTC: 100, 100, 100, 90 (init false), 120 (flip true)
        // ETH: 100, 100, 100, 120 (init true), 70 (flip false)
        let sequence = [
            ("BTC", 100.0),
            ("ETH", 100.0),
            ("BTC", 100.0),
            ("ETH", 100.0),
            ("BTC", 100.0),
            ("ETH", 100.0),
            ("BTC", 90.0),
            ("ETH", 120.0),
            ("BTC", 120.0),
            ("ETH", 70.0),
        ];

        for (i, (sym, p)) in sequence.iter().enumerate() {
            let candle = Candle {
                symbol: sym.to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        let mut btc_buy = false;
        let mut eth_sell = false;

        for _ in 0..5 {
            if let Ok(Some(proposal)) =
                tokio::time::timeout(std::time::Duration::from_millis(100), proposal_rx.recv())
                    .await
            {
                if proposal.symbol == "BTC" && proposal.side == OrderSide::Buy {
                    btc_buy = true;
                }
                if proposal.symbol == "ETH" && proposal.side == OrderSide::Sell {
                    eth_sell = true;
                }
            }
        }

        assert!(btc_buy, "Should receive BTC buy signal");
        assert!(eth_sell, "Should receive ETH sell signal");
    }

    #[tokio::test]
    async fn test_advanced_strategy_trend_filter() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);
        let portfolio = Arc::new(RwLock::new(crate::domain::portfolio::Portfolio::new()));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio,
        ));

        // Advanced mode with long trend SMA
        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::config::StrategyMode::Advanced,
            trend_sma_period: 10, // Long trend
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 55.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            max_position_size_pct: 0.1,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(market_rx, proposal_tx, exec_service, strategy, config);

        tokio::spawn(async move {
            analyst.run().await;
        });

        // Prices are low, but SMA cross happens. Trend (SMA 10) will be around 50.
        // Fast/Slow cross happens at 45 -> 55.
        let prices = [50.0, 50.0, 50.0, 45.0, 55.0];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "AAPL".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        // Should NOT receive buy signal because price (55) is likely not ABOVE the trend SMA yet
        // OR RSI filter prevents it if it's too volatile.
        // Actually, with these prices, trend SMA will be < 55.
        // Let's make price definitely BELOW trend.
        // Prices: 100, 100, 100, 90, 95. Trend SMA will be ~97. Current Price 95 < 97.
        let prices2 = [100.0, 100.0, 100.0, 90.0, 95.0];
        for (i, p) in prices2.iter().enumerate() {
            let candle = Candle {
                symbol: "MSFT".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100,
                timestamp: (i + 10) as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        let mut received = false;
        while let Ok(Some(_)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), proposal_rx.recv()).await
        {
            received = true;
        }
        assert!(
            !received,
            "Should NOT receive signal when trend filter rejects it"
        );
    }

    #[tokio::test]
    async fn test_risk_based_quantity_calculation() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);
        
        // Start with empty portfolio - this is the production issue scenario
        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0); // $100,000 starting cash
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        // Production-like configuration
        let config = AnalystConfig {
            fast_sma_period: 20,
            slow_sma_period: 60,
            max_positions: 5,
            trade_quantity: Decimal::from(1), // Fallback if risk sizing not used
            sma_threshold: 0.0005,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.01, // 1% of equity per trade
            strategy_mode: crate::config::StrategyMode::Dynamic,
            trend_sma_period: 200,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 65.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.001,
            max_position_size_pct: 0.1, // 10% maximum position size
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(market_rx, proposal_tx, exec_service, strategy, config);

        tokio::spawn(async move {
            analyst.run().await;
        });

        // Generate a golden cross scenario
        // Start low, then cross up
        let prices = vec![
            100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0,
            100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0,
            102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0, 110.0, 111.0,
            112.0, 113.0, 114.0, 115.0, 116.0, 117.0, 118.0, 119.0, 120.0, 121.0,
            122.0, 123.0, 124.0, 125.0, 126.0, 127.0, 128.0, 129.0, 130.0, 131.0,
            132.0, 133.0, 134.0, 135.0, 136.0, 137.0, 138.0, 139.0, 140.0, 141.0,
            142.0, 143.0, 144.0, 145.0,
        ];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "NVDA".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 1000000,
                timestamp: (i * 1000) as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        // Should receive at least one buy signal
        let proposal = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            proposal_rx.recv()
        )
        .await
        .expect("Should receive a proposal within timeout")
        .expect("Should receive a buy signal");

        assert_eq!(proposal.side, OrderSide::Buy, "Should generate a buy signal");
        
        // Verify quantity is calculated based on risk, not static value
        // With $100,000 equity, 1% risk = $1,000
        // At price ~140, quantity should be around 1000/140 = ~7 shares
        // But also capped by max_position_size_pct of 10% = $10,000 / 140 = ~71 shares
        // And by max_positions bucket: $100,000 / 5 = $20,000 / 140 = ~142 shares
        // So we expect: min(1000/140, 10000/140, 20000/140) ≈ 7.14 shares
        assert!(
            proposal.quantity > Decimal::ZERO,
            "Quantity should be greater than zero (was {})",
            proposal.quantity
        );
        assert!(
            proposal.quantity > Decimal::from(1),
            "Quantity should be risk-based, not the static fallback of 1 share (was {})",
            proposal.quantity
        );
        assert!(
            proposal.quantity < Decimal::from(100),
            "Quantity should be reasonable (was {})",
            proposal.quantity
        );
    }
}
