use crate::application::market_data::candle_aggregator::CandleAggregator;
use crate::application::market_data::signal_generator::SignalGenerator;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::cost_evaluator::CostEvaluator;
use crate::application::monitoring::feature_engineering_service::TechnicalFeatureEngineeringService;
use crate::application::optimization::expectancy_evaluator::MarketExpectancyEvaluator;
use crate::application::optimization::win_rate_provider::{StaticWinRateProvider, WinRateProvider};
use crate::application::risk_management::position_manager::PositionManager;

use crate::application::risk_management::trailing_stops::StopState;

use crate::application::strategies::strategy_selector::StrategySelector;
use crate::application::strategies::{StrategyFactory, TradingStrategy};

use crate::domain::market::market_regime::{MarketRegime, MarketRegimeDetector};
use crate::domain::ports::{
    ExecutionService, ExpectancyEvaluator, FeatureEngineeringService, MarketDataService,
};
use crate::domain::repositories::{CandleRepository, StrategyRepository};
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::OrderStatus; // Added
use crate::domain::trading::types::{FeatureSet, MarketEvent, OrderSide, TradeProposal};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};

pub struct SymbolContext {
    pub feature_service: Box<dyn FeatureEngineeringService>,
    pub signal_generator: SignalGenerator,
    pub position_manager: PositionManager,
    pub strategy: Arc<dyn TradingStrategy>, // Per-symbol strategy
    pub config: AnalystConfig,              // Per-symbol config
    pub last_features: FeatureSet,          // Primary timeframe features
    pub regime_detector: MarketRegimeDetector,
    pub expectancy_evaluator: Box<dyn ExpectancyEvaluator>,
    pub taken_profit: bool, // Track if partial profit has been taken for current position
    pub last_entry_time: Option<i64>, // Phase 2: track entry time for min hold
    pub min_hold_time_ms: i64, // Phase 2: minimum hold time in milliseconds
    pub active_strategy_mode: crate::domain::market::strategy_config::StrategyMode, // Phase 3: Track active mode
    pub last_macd_histogram: Option<f64>, // Track previous MACD histogram for rising/falling detection
    pub cached_reward_risk_ratio: f64,    // Calculated during warmup, used for trade filtering
    pub warmup_succeeded: bool,           // Track if historical warmup was successful
    // Multi-timeframe support
    pub timeframe_aggregator: crate::application::market_data::timeframe_aggregator::TimeframeAggregator,
    pub timeframe_features: std::collections::HashMap<crate::domain::market::timeframe::Timeframe, FeatureSet>,
    pub enabled_timeframes: Vec<crate::domain::market::timeframe::Timeframe>,
}

#[derive(Debug)]
pub enum AnalystCommand {
    UpdateConfig(Box<AnalystConfig>),
}


impl SymbolContext {
    pub fn new(
        config: AnalystConfig,
        strategy: Arc<dyn TradingStrategy>,
        win_rate_provider: Arc<dyn WinRateProvider>,
        enabled_timeframes: Vec<crate::domain::market::timeframe::Timeframe>,
    ) -> Self {
        let min_hold_time_ms = config.min_hold_time_minutes * 60 * 1000;

        Self {
            feature_service: Box::new(TechnicalFeatureEngineeringService::new(&config)),
            signal_generator: SignalGenerator::new(),
            position_manager: PositionManager::new(),
            strategy,
            config: config.clone(),
            last_features: FeatureSet::default(),

            regime_detector: MarketRegimeDetector::new(20, 25.0, 2.0), // Default thresholds
            expectancy_evaluator: Box::new(MarketExpectancyEvaluator::new(1.5, win_rate_provider)),
            taken_profit: false,
            last_entry_time: None,
            min_hold_time_ms,
            active_strategy_mode: config.strategy_mode, // Initial mode
            last_macd_histogram: None,
            cached_reward_risk_ratio: 1.0, // Default safe value, will be updated during warmup
            warmup_succeeded: false,       // Will be set to true if warmup completes
            // Multi-timeframe initialization
            timeframe_aggregator: crate::application::market_data::timeframe_aggregator::TimeframeAggregator::new(),
            timeframe_features: std::collections::HashMap::new(),
            enabled_timeframes,
        }
    }

    pub fn update(&mut self, candle: &crate::domain::trading::types::Candle) {
        // Store previous MACD histogram before updating features
        self.last_macd_histogram = self.last_features.macd_hist;
        self.last_features = self.feature_service.update(candle);
    }
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
    pub strategy_mode: crate::domain::market::strategy_config::StrategyMode,
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
    pub commission_per_share: f64, // Added
    pub max_position_size_pct: f64,
    pub bb_period: usize,
    pub bb_std_dev: f64,
    pub macd_fast: usize,
    pub macd_slow: usize,
    pub macd_signal: usize,
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,
    pub take_profit_pct: f64,
    pub min_hold_time_minutes: i64,      // Phase 2: minimum hold time
    pub signal_confirmation_bars: usize, // Phase 2: signal confirmation
    pub spread_bps: f64,                 // Cost-aware trading: spread in basis points
    pub min_profit_ratio: f64,           // Cost-aware trading: minimum profit/cost ratio
    pub profit_target_multiplier: f64,
    // Risk-based adaptive filters
    pub macd_requires_rising: bool, // Whether MACD must be rising for buy signals
    pub trend_tolerance_pct: f64,   // Percentage tolerance for trend filter
    pub macd_min_threshold: f64,    // Minimum MACD histogram threshold
    pub adx_period: usize,
    pub adx_threshold: f64,
}

impl Default for AnalystConfig {
    fn default() -> Self {
        Self {
            fast_sma_period: 10,
            slow_sma_period: 20,
            max_positions: 5,
            trade_quantity: rust_decimal::Decimal::ONE,
            sma_threshold: 0.05,
            order_cooldown_seconds: 60,
            risk_per_trade_percent: 1.0,
            strategy_mode: Default::default(),
            trend_sma_period: 50,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.05,
            trailing_stop_atr_multiplier: 2.0,
            atr_period: 14,
            rsi_threshold: 70.0,
            trend_riding_exit_buffer_pct: 0.02,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            commission_per_share: 0.0,
            max_position_size_pct: 10.0,
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 10,
            ema_slow_period: 20,
            take_profit_pct: 0.1,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 0.0,
            min_profit_ratio: 1.5,
            profit_target_multiplier: 2.0,
            macd_requires_rising: false,
            trend_tolerance_pct: 0.02,
            macd_min_threshold: 0.0,
            adx_period: 14,
            adx_threshold: 25.0,
        }
    }
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
            commission_per_share: config.commission_per_share, // Added
            max_position_size_pct: config.max_position_size_pct,
            bb_period: config.mean_reversion_bb_period,
            bb_std_dev: 2.0,
            macd_fast: config.macd_fast_period,
            macd_slow: config.macd_slow_period,
            macd_signal: config.macd_signal_period,
            ema_fast_period: config.ema_fast_period,
            ema_slow_period: config.ema_slow_period,
            take_profit_pct: config.take_profit_pct,
            min_hold_time_minutes: config.min_hold_time_minutes,
            signal_confirmation_bars: config.signal_confirmation_bars,
            spread_bps: config.spread_bps,
            min_profit_ratio: config.min_profit_ratio,
            profit_target_multiplier: config.profit_target_multiplier,
            macd_requires_rising: config.macd_requires_rising,
            trend_tolerance_pct: config.trend_tolerance_pct,
            macd_min_threshold: config.macd_min_threshold,
            adx_period: config.adx_period,
            adx_threshold: config.adx_threshold,
        }
    }
}

impl From<&AnalystConfig> for crate::application::risk_management::sizing_engine::SizingConfig {
    fn from(config: &AnalystConfig) -> Self {
        Self {
            risk_per_trade_percent: config.risk_per_trade_percent,
            max_positions: config.max_positions,
            max_position_size_pct: config.max_position_size_pct,
            static_trade_quantity: config.trade_quantity,
        }
    }
}

pub struct AnalystDependencies {
    pub execution_service: Arc<dyn ExecutionService>,
    pub market_service: Arc<dyn MarketDataService>,
    pub candle_repository: Option<Arc<dyn CandleRepository>>,
    pub strategy_repository: Option<Arc<dyn StrategyRepository>>,
    pub win_rate_provider: Option<Arc<dyn WinRateProvider>>,
    pub ui_candle_tx: Option<broadcast::Sender<Candle>>,
    pub spread_cache: Arc<SpreadCache>, // NEW: For real-time cost calculation
}

pub struct Analyst {
    market_rx: Receiver<MarketEvent>,
    proposal_tx: Sender<TradeProposal>,
    execution_service: Arc<dyn ExecutionService>,
    market_service: Arc<dyn MarketDataService>, // Added for warmup
    default_strategy: Arc<dyn TradingStrategy>, // Fallback
    config: AnalystConfig,                      // Default config
    symbol_states: HashMap<String, SymbolContext>,
    candle_aggregator: CandleAggregator,
    candle_repository: Option<Arc<dyn CandleRepository>>,
    strategy_repository: Option<Arc<dyn StrategyRepository>>, // Added
    win_rate_provider: Arc<dyn WinRateProvider>,              // Added
    ui_candle_tx: Option<broadcast::Sender<Candle>>,          // Added for UI streaming

    trade_filter: crate::application::trading::trade_filter::TradeFilter,
    // Multi-timeframe configuration
    enabled_timeframes: Vec<crate::domain::market::timeframe::Timeframe>,
    cmd_rx: Receiver<AnalystCommand>,
}



impl Analyst {
    pub fn new(
        market_rx: Receiver<MarketEvent>,
        cmd_rx: Receiver<AnalystCommand>,
        proposal_tx: Sender<TradeProposal>,
        config: AnalystConfig,
        default_strategy: Arc<dyn TradingStrategy>,
        dependencies: AnalystDependencies,
    ) -> Self {
        // Default to Static 50% if not provided (Conservative baseline)
        let win_rate_provider = dependencies
            .win_rate_provider
            .unwrap_or_else(|| Arc::new(StaticWinRateProvider::new(0.50)));

        // Initialize Cost Evaluator for profit-aware trading WITH real-time spreads
        let cost_evaluator = CostEvaluator::with_spread_cache(
            config.commission_per_share,
            config.slippage_pct,
            config.spread_bps, // Default fallback if real spread unavailable
            dependencies.spread_cache.clone(), // Real-time spreads from WebSocket!
        );

        let trade_filter =
            crate::application::trading::trade_filter::TradeFilter::new(cost_evaluator.clone());

        // Extract enabled timeframes from config (will be passed from system.rs)
        // For now, default to primary timeframe only to maintain backward compatibility
        let enabled_timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        Self {
            market_rx,
            proposal_tx,
            execution_service: dependencies.execution_service,
            market_service: dependencies.market_service,
            default_strategy,
            config,
            symbol_states: HashMap::new(),
            candle_aggregator: CandleAggregator::new(
                dependencies.candle_repository.clone(),
                dependencies.spread_cache.clone(),
            ),
            candle_repository: dependencies.candle_repository,
            strategy_repository: dependencies.strategy_repository,
            win_rate_provider,
            ui_candle_tx: dependencies.ui_candle_tx,
            trade_filter,
            enabled_timeframes,
            cmd_rx,
        }
    }

    pub async fn run(&mut self) {
        info!(
            "Analyst started (Multi-Symbol Dual SMA). Cache size: {}",
            self.config.max_positions
        );

        // Subscribe to Order Updates
        let mut order_rx = match self.execution_service.subscribe_order_updates().await {
            Ok(rx) => {
                info!("Analyst: Subscribed to order updates.");
                Some(rx)
            }
            Err(e) => {
                error!("Analyst: Failed to subscribe to order updates: {}", e);
                None
            }
        };

        loop {
            tokio::select! {
                Some(event) = self.market_rx.recv() => {
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
                        MarketEvent::SymbolSubscription { symbol } => {
                            info!("Analyst: Received immediate warmup request for {}", symbol);
                            self.ensure_symbol_initialized(&symbol, chrono::Utc::now()).await;
                        }
                    }
                }

                // Handle Order Updates
                Ok(order_update) = async {
                     if let Some(rx) = &mut order_rx {
                         rx.recv().await
                     } else {
                         std::future::pending().await // Wait forever if no subscription
                     }
                } => {
                    debug!("Analyst: Received Order Update for {}: {:?}", order_update.symbol, order_update.status);

                    if let Some(context) = self.symbol_states.get_mut(&order_update.symbol) {
                         // If order is Filled or Canceled, we clear the pending state immediately
                         match order_update.status {
                             OrderStatus::Filled | OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Rejected => {
                                 info!("Analyst: Order {} for {} resolved ({:?}). Clearing pending state.", order_update.order_id, order_update.symbol, order_update.status);
                                 context.position_manager.clear_pending();
                             }
                             _ => {}
                         }
                    }
                }

                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        AnalystCommand::UpdateConfig(new_config) => {
                            info!("Analyst: Updating configuration...");
                            self.config = *new_config;
                            // Propagate to all existing symbol contexts
                            for context in self.symbol_states.values_mut() {
                                context.config = self.config.clone();
                                // Check if structural parameters changed (periods)
                                if context.config.rsi_period != self.config.rsi_period || 
                                   context.config.fast_sma_period != self.config.fast_sma_period ||
                                   context.config.slow_sma_period != self.config.slow_sma_period {
                                     warn!("Analyst: Structural config change detected. Re-initializing Feature Service.");
                                     context.feature_service = Box::new(crate::application::monitoring::feature_engineering_service::TechnicalFeatureEngineeringService::new(&self.config));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ============================================================================
    // PIPELINE HANDLERS (Phase 2.2)
    // ============================================================================

    /// Detect market regime for current symbol
    async fn detect_market_regime(
        repo: &Option<Arc<dyn CandleRepository>>,
        symbol: &str,
        candle_timestamp: i64,
        context: &SymbolContext,
    ) -> MarketRegime {
        if let Some(repo) = repo {
            let end_ts = candle_timestamp;
            let start_ts = end_ts - (30 * 24 * 60 * 60); // 30 days
            
            if let Ok(candles) = repo.get_range(symbol, start_ts, end_ts).await {
                return context
                    .regime_detector
                    .detect(&candles)
                    .unwrap_or(MarketRegime::unknown());
            }
        }
        MarketRegime::unknown()
    }

    /// Generate trading signal from strategy
    fn generate_trading_signal(
        context: &mut SymbolContext,
        symbol: &str,
        price: Decimal,
        timestamp: i64,
        has_position: bool,
    ) -> Option<OrderSide> {
        context.signal_generator.generate_signal(
            symbol,
            price,
            timestamp,
            &context.last_features,
            &context.strategy,
            context.config.sma_threshold,
            has_position,
            context.last_macd_histogram,
        )
    }

    /// Build trade proposal from signal
    async fn build_trade_proposal(
        config: &AnalystConfig,
        execution_service: &Arc<dyn ExecutionService>,
        symbol: String,
        side: OrderSide,
        price: Decimal,
        timestamp: i64,
        reason: String,
    ) -> Option<TradeProposal> {
        // Calculate quantity
        let quantity = Self::calculate_trade_quantity(
            config,
            execution_service,
            &symbol,
            price,
        )
        .await;

        if quantity <= Decimal::ZERO {
            debug!("Analyst [{}]: Quantity is ZERO. Skipping proposal.", symbol);
            return None;
        }

        Some(TradeProposal {
            symbol,
            side,
            price,
            quantity,
            order_type: crate::domain::trading::types::OrderType::Market,
            reason,
            timestamp,
        })
    }

    async fn process_candle(&mut self, candle: crate::domain::trading::types::Candle) {
        let symbol = candle.symbol.clone();
        let price = candle.close;
        let timestamp = candle.timestamp * 1000;
        let price_f64 = price.to_f64().unwrap_or(0.0);

        // Broadcast to UI
        if let Some(tx) = &self.ui_candle_tx {
            match tx.send(candle.clone()) {
                Ok(_) => debug!(
                    "Analyst: Broadcasted candle for {} (price: {}) to UI",
                    symbol, price
                ),
                Err(e) => warn!("Analyst: Failed to broadcast candle to UI: {}", e),
            }
        } else {
            warn!("Analyst: No UI candle broadcaster configured!");
        }

        // 1. Get/Init Context (Consolidated with ensure_symbol_initialized)
        let timestamp_dt = chrono::DateTime::from_timestamp(candle.timestamp, 0)
            .unwrap_or_default()
            .with_timezone(&chrono::Utc);
        
        self.ensure_symbol_initialized(&symbol, timestamp_dt).await;

        let context = self.symbol_states.get_mut(&symbol).unwrap();

        // 1.5 Detect Market Regime
        let regime = Self::detect_market_regime(&self.candle_repository, &symbol, candle.timestamp, context).await;

        // 1.6 Adaptive Strategy Switching (Phase 3)
        if context.config.strategy_mode
            == crate::domain::market::strategy_config::StrategyMode::RegimeAdaptive
        {
            let (new_mode, new_strategy) = StrategySelector::select_strategy(
                &regime,
                &context.config,
                context.active_strategy_mode,
            );

            if new_mode != context.active_strategy_mode {
                info!(
                    "Analyst: Adaptive Switch for {} -> {:?} (Regime: {:?})",
                    symbol, new_mode, regime.regime_type
                );
                context.strategy = new_strategy;
                context.active_strategy_mode = new_mode;
            }
        }

        // 2. Update Indicators via Service
        context.update(&candle);

        // 3. Sync with Portfolio

        let portfolio_res = self.execution_service.get_portfolio().await;
        let portfolio_data = portfolio_res.as_ref().ok();

        if let Some(portfolio) = portfolio_data {
            let has_position = portfolio
                .positions
                .get(&symbol)
                .map(|p| p.quantity > Decimal::ZERO)
                .unwrap_or(false);
            context
                .position_manager
                .ack_pending_orders(has_position, &symbol);
        }

        let has_position = portfolio_data
            .map(|p| {
                p.positions
                    .get(&symbol)
                    .map(|pos| pos.quantity > Decimal::ZERO)
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if !has_position {
            context.taken_profit = false;
        }

        // 4. Check Trailing Stop (Priority Exit) via PositionManager
        let mut signal = context.position_manager.check_trailing_stop(
            &symbol,
            price_f64,
            context.last_features.atr.unwrap_or(0.0),
            context.config.trailing_stop_atr_multiplier,
        );
        let trailing_stop_triggered = signal.is_some();

        // Check Partial Take-Profit (Swing Trading Upgrade)
        if !trailing_stop_triggered && has_position
            && let Some(portfolio) = portfolio_data
                && let Some(pos) = portfolio.positions.get(&symbol)
                    && pos.quantity > Decimal::ZERO {
                        let avg_price = pos.average_price.to_f64().unwrap_or(1.0);
                        let pnl_pct = (price_f64 - avg_price) / avg_price;

                        // Check if we hit profit target and haven't taken profit yet
                        if pnl_pct >= context.config.take_profit_pct && !context.taken_profit {
                            let quantity_to_sell = (pos.quantity * Decimal::new(5, 1)).round_dp(4); // 50%

                            if quantity_to_sell > Decimal::ZERO {
                                info!("Analyst: Triggering Partial Take-Profit (50%) for {} at {:.2}% Gain", symbol, pnl_pct * 100.0);

                                let proposal = TradeProposal {
                                    symbol: symbol.clone(),
                                    side: OrderSide::Sell,
                                    price: Decimal::from_f64_retain(price_f64).unwrap(),
                                    quantity: quantity_to_sell,
                                    order_type: crate::domain::trading::types::OrderType::Market,
                                    reason: format!(
                                        "Partial Take-Profit (+{:.2}%)",
                                        pnl_pct * 100.0
                                    ),
                                    timestamp,
                                };

                                match self.proposal_tx.try_send(proposal) {
                                    Ok(_) => {
                                        context.taken_profit = true;
                                        // Don't process further signals this tick
                                        return;
                                    }
                                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                        warn!("Analyst [{}]: Proposal channel FULL - RiskManager slow. Backpressure applied, skipping proposal.", symbol);
                                        return;
                                    }
                                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                        error!(
                                            "Analyst [{}]: Proposal channel CLOSED. Shutting down.",
                                            symbol
                                        );
                                        return;
                                    }
                                }
                            }
                        }
                    }

        // Monitor pending order timeout
        if context.position_manager.check_timeout(timestamp, 60000) {
            // 60s timeout
            info!(
                "Analyst [{}]: Pending order TIMEOUT detected. Checking open orders to CANCEL...",
                symbol
            );

            // 1. Fetch Open Orders
            match self.execution_service.get_open_orders().await {
                Ok(orders) => {
                    // 2. Find orders for this symbol
                    let symbol_orders: Vec<_> =
                        orders.iter().filter(|o| o.symbol == symbol).collect();

                    if symbol_orders.is_empty() {
                        info!("Analyst [{}]: No open orders found on exchange. Clearing local pending state.", symbol);
                        context.position_manager.clear_pending();
                    } else {
                        // 3. Cancel them
                        for order in symbol_orders {
                            info!(
                                "Analyst [{}]: Cancelling orphaned order {}...",
                                symbol, order.id
                            );
                            if let Err(e) = self.execution_service.cancel_order(&order.id).await {
                                error!(
                                    "Analyst [{}]: Failed to cancel order {}: {}",
                                    symbol, order.id, e
                                );
                            }
                        }
                        // 4. Clear local state (Optimistic or wait for update)
                        context.position_manager.clear_pending();
                    }
                }
                Err(e) => {
                    error!(
                        "Analyst [{}]: Failed to fetch open orders for cancellation check: {}",
                        symbol, e
                    );
                }
            }
        }

        // 5. Generate Trading Signal
        if !trailing_stop_triggered {
            signal = Self::generate_trading_signal(
                context,
                &symbol,
                price,
                timestamp,
                has_position,
            );

            // RSI Filtering (Strategic Tuning)
            if let Some(OrderSide::Buy) = signal
                && let Some(rsi) = context.last_features.rsi
                    && rsi > context.config.rsi_threshold {
                        info!(
                            "Analyst: Buy signal BLOCKED for {} - RSI {:.2} > {:.2} (Overbought)",
                            symbol, rsi, context.config.rsi_threshold
                        );
                        signal = None;
                    }

            // Suppress SMA-cross sell if Trailing Stop is active
            if let Some(OrderSide::Sell) = signal
                && context.position_manager.trailing_stop.is_active() && !trailing_stop_triggered {
                    info!(
                        "Analyst: Sell signal SUPPRESSED for {} - Using trailing stop exit instead",
                        symbol
                    );
                    signal = None;
                }
        }

        // 6. Post-Signal Validation (Long-Only, Pending, Cooldown)
        if let Some(side) = signal {
            // DELEGATED TO TRADE FILTER
            if !self.trade_filter.validate_signal(
                side,
                &symbol,
                &context.position_manager,
                &context.config,
                timestamp,
                has_position,
            ) {
                return;
            }

            // 7. Execution Logic (Expectancy & Quantity)
            context.position_manager.last_signal_time = timestamp;

            // Use already calculated regime for expectancy
            let expectancy = context
                .expectancy_evaluator
                .evaluate(&symbol, price, &regime)
                .await;

            let risk_ratio = if expectancy.reward_risk_ratio > 0.0 {
                expectancy.reward_risk_ratio
            } else {
                context.cached_reward_risk_ratio
            };

            // Validate using calculated or cached ratio
            if !self.trade_filter.validate_expectancy(&symbol, risk_ratio) {
                return;
            }

            // Phase 2: Check minimum hold time for sell signals
            if !self.trade_filter.validate_min_hold_time(
                side,
                &symbol,
                timestamp,
                context.last_entry_time,
                context.min_hold_time_ms,
            ) {
                return;
            }

            let order_type = match side {
                OrderSide::Buy => crate::domain::trading::types::OrderType::Limit,
                OrderSide::Sell => crate::domain::trading::types::OrderType::Market,
            };

            let reason = format!(
                "Strategy: {} (Regime: {})",
                context.active_strategy_mode, regime.regime_type
            );

            let mut proposal = match Self::build_trade_proposal(
                &context.config,
                &self.execution_service,
                symbol.clone(),
                side,
                price,
                timestamp,
                reason,
            ).await {
                Some(p) => p,
                None => return,
            };
            
            proposal.order_type = order_type;

            // ============ COST-AWARE TRADING FILTER ============
            let atr = context.last_features.atr.unwrap_or(0.0);
            
            info!(
                "Analyst [{}]: Calculating Profit Expectancy - ATR=${:.4}, Multiplier={:.2}, Quantity={}",
                symbol, atr, context.config.profit_target_multiplier, proposal.quantity
            );

            // Use fresh expectancy value if available
            let expected_profit = if expectancy.expected_value > 0.0 {
                Decimal::from_f64_retain(expectancy.expected_value).unwrap_or(Decimal::ZERO) * proposal.quantity
            } else {
                self.trade_filter.calculate_expected_profit(
                    &proposal,
                    atr,
                    context.config.profit_target_multiplier,
                )
            };

            let costs = self.trade_filter.evaluate_costs(&proposal);
            
            if !self.trade_filter.validate_profitability(
                &proposal,
                expected_profit,
                costs.total_cost,
                context.config.min_profit_ratio,
                &symbol,
            ) {
                return;
            }
                // ====================================================

                match self.proposal_tx.try_send(proposal) {
                    Ok(_) => {
                        context.position_manager.set_pending_order(side, timestamp);

                        // Phase 2: Track entry time on buy signals
                        if side == OrderSide::Buy {
                            context.last_entry_time = Some(timestamp);
                        }
                        if side == OrderSide::Buy
                            && let Some(atr) = context.last_features.atr
                                && atr > 0.0 {
                                    context.position_manager.trailing_stop = StopState::on_buy(
                                        price_f64,
                                        atr,
                                        context.config.trailing_stop_atr_multiplier,
                                    );
                                }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        warn!(
                            "Analyst [{}]: Proposal channel FULL - RiskManager slow. Backpressure applied, proposal dropped.",
                            symbol
                        );
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                        error!(
                            "Analyst [{}]: Proposal channel CLOSED. Shutting down.",
                            symbol
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
        // Get Total Equity from Portfolio
        // If fail, we can only default to static if risk-sizing is NOT required
        // But SizingEngine needs equity.

        let mut total_equity = Decimal::ZERO;

        if config.risk_per_trade_percent > 0.0 {
            let portfolio_result = execution_service.get_portfolio().await;
            if let Ok(portfolio) = portfolio_result {
                total_equity = portfolio.cash;
                for pos in portfolio.positions.values() {
                    total_equity += pos.quantity * pos.average_price;
                }
            } else {
                info!("Analyst: Failed to get portfolio for sizing. Defaulting to 0 equity (will result in 0 quantity if risk-sizing enabled).");
            }
        }

        let sizing_config: crate::application::risk_management::sizing_engine::SizingConfig =
            config.into();

        crate::application::risk_management::sizing_engine::SizingEngine::calculate_quantity(
            &sizing_config,
            total_equity,
            price,
            symbol,
        )
    }
    async fn resolve_strategy(&self, symbol: &str) -> (Arc<dyn TradingStrategy>, AnalystConfig) {
        if let Some(repo) = &self.strategy_repository
            && let Ok(Some(def)) = repo.find_by_symbol(symbol).await {
                let mut config = self.config.clone();

                if let Ok(parsed_config) = serde_json::from_str::<AnalystConfig>(&def.config_json) {
                    config = parsed_config;
                    debug!("Analyst: Loaded custom config for {}", symbol);
                } else {
                    debug!("Analyst: Failed to parse full config for {}, using default with custom strategy", symbol);
                }

                config.strategy_mode = def.mode;

                let strategy = StrategyFactory::create(def.mode, &config);
                return (strategy, config);
            }

        // Default
        (self.default_strategy.clone(), self.config.clone())
    }

    async fn warmup_context(
        &self,
        context: &mut SymbolContext,
        symbol: &str,
        end: chrono::DateTime<chrono::Utc>,
    ) {
        // Calculate needed lookback
        // Max(TrendSMA, SlowSMA, EMA, RSI, MACD_Slow)
        let config = &context.config;
        let max_period = [
            config.trend_sma_period,
            config.slow_sma_period,
            config.ema_slow_period,
            config.rsi_period * 2, // General rule for RSI stability
            config.macd_slow_period + config.macd_signal_period,
        ]
        .iter()
        .max()
        .copied()
        .unwrap_or(200);

        // Add 10% buffer
        let required_bars = (max_period as f64 * 1.1) as usize;

        info!(
            "Analyst: Warming up {} with {} bars (Max Period: {}) ending at {}",
            symbol, required_bars, max_period, end
        );

        // Assuming 1-minute bars.
        // Assuming 1-minute bars.
        // Market is open 6.5h a day ~ 390mins.
        // 2000 bars is ~5.1 trading days.
        // We fetch enough calendar days back to cover weekends/holidays (e.g., 2000 bars might need 10 days if over weekend).
        // Let's use a safe multiplier.
        let days_back = (required_bars / (300)) + 3;
        let start = end - chrono::Duration::days(days_back as i64);

        match self
            .market_service
            .get_historical_bars(symbol, start, end, "1Min")
            .await
        {
            Ok(bars) => {
                let bars_count = bars.len();
                info!(
                    "Analyst: Fetched {} historical bars for {}",
                    bars_count, symbol
                );

                for candle in &bars {
                    // Update context (features + indicators)
                    context.update(candle);
                }

                debug!(
                    "Analyst: Warmup complete for {} with {} candles. Last Price: {:?}",
                    symbol,
                    bars.len(),
                    context.last_features.sma_50
                );

                // Calculate and cache reward/risk ratio for trade filtering
                if !bars.is_empty() {
                    let regime = context
                        .regime_detector
                        .detect(&bars)
                        .unwrap_or(MarketRegime::unknown());
                    let last_price_decimal = bars.last().unwrap().close;

                    let expectancy = context
                        .expectancy_evaluator
                        .evaluate(symbol, last_price_decimal, &regime)
                        .await;
                    context.cached_reward_risk_ratio = expectancy.reward_risk_ratio;

                    info!(
                        "Analyst: Cached reward/risk ratio for {}: {:.2}",
                        symbol, context.cached_reward_risk_ratio
                    );
                }

                // Broadcast last 100 historical candles to UI for chart initialization
                if let Some(tx) = &self.ui_candle_tx {
                    let start_idx = bars.len().saturating_sub(100);
                    let recent_bars = &bars[start_idx..];
                    info!(
                        "Analyst: Broadcasting {} historical candles for {} to UI",
                        recent_bars.len(),
                        symbol
                    );

                    for bar in recent_bars {
                        let candle = crate::domain::trading::types::Candle {
                            symbol: symbol.to_string(),
                            open: bar.open,
                            high: bar.high,
                            low: bar.low,
                            close: bar.close,
                            volume: bar.volume,
                            timestamp: bar.timestamp,
                        };
                        let _ = tx.send(candle);
                    }
                }

                // Mark warmup as successful
                context.warmup_succeeded = true;
                info!(
                    "Analyst: ✓ Warmup completed successfully for {} with {} bars",
                    symbol,
                    bars.len()
                );
            }
            Err(e) => {
                warn!(
                    "Analyst: Failed to warmup {}: {}. Indicators will start from zero (degraded mode)",
                    symbol, e
                );
                // warmup_succeeded remains false
                // Indicators are already initialized to zero/default in SymbolContext::new()
                // The system will continue trading but with less historical context
            }
        }
    }

    async fn ensure_symbol_initialized(
        &mut self,
        symbol: &str,
        end_time: chrono::DateTime<chrono::Utc>,
    ) {
        if !self.symbol_states.contains_key(symbol) {
            info!(
                "Analyst: Initializing context for {} (Warmup end: {})",
                symbol, end_time
            );
            let (strategy, config) = self.resolve_strategy(symbol).await;
            let mut context = SymbolContext::new(
                config,
                strategy,
                self.win_rate_provider.clone(),
                self.enabled_timeframes.clone(),
            );

            // WARMUP: Fetch historical data to initialize indicators
            self.warmup_context(&mut context, symbol, end_time).await;

            self.symbol_states.insert(symbol.to_string(), context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::Candle;
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
    async fn test_immediate_warmup() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, _proposal_rx) = mpsc::channel(10);

        use crate::domain::trading::portfolio::Portfolio;
        let portfolio = Portfolio::new();
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());
        let config = AnalystConfig::default();
        let strategy = crate::application::strategies::StrategyFactory::create(
            crate::domain::market::strategy_config::StrategyMode::Advanced,
            &config,
        );

        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(
                    crate::application::market_data::spread_cache::SpreadCache::new(),
                ),
            },
        );

        // Send subscription event
        market_tx
            .send(MarketEvent::SymbolSubscription {
                symbol: "BTC/USD".to_string(),
            })
            .await
            .unwrap();

        // Run analyst briefly
        tokio::select! {
            _ = analyst.run() => {},
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
        }

        // Check if context was created
        assert!(analyst.symbol_states.contains_key("BTC/USD"));
        let _context = analyst.symbol_states.get("BTC/USD").unwrap();
        // Warmup should have been attempted (even if it yielded 0 bars in mock)
        // We can't easily check internal warmup state without exposing it, 
        // but the presence of the context proves the branch was hit.
    }

    #[tokio::test]
    async fn test_golden_cross() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        use crate::domain::trading::portfolio::Portfolio;
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::from(100000);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());
        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::domain::market::strategy_config::StrategyMode::Standard,
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 99.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            commission_per_share: 0.0,
            max_position_size_pct: 0.0,
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: 0.05,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 5.0,
            min_profit_ratio: 2.0,

            macd_requires_rising: true,

            trend_tolerance_pct: 0.0,

            macd_min_threshold: 0.0,

            profit_target_multiplier: 1.5,
            adx_period: 14,
            adx_threshold: 25.0,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(SpreadCache::new()),
            },
        );

        tokio::spawn(async move {
            analyst.run().await;
        });

        use crate::domain::trading::types::Candle;

        // Dual SMA (2, 3)
        let prices = [100.0, 100.0, 100.0, 90.0, 110.0, 120.0];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "BTC".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100.0,
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
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        use crate::domain::trading::portfolio::Portfolio;
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::from(100000);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));
        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());

        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::domain::market::strategy_config::StrategyMode::Standard,
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
            commission_per_share: 0.0,
            max_position_size_pct: 0.1,
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: 0.05,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 5.0,
            min_profit_ratio: 2.0,

            macd_requires_rising: true,

            trend_tolerance_pct: 0.0,

            macd_min_threshold: 0.0,
            profit_target_multiplier: 1.5,
            adx_period: 14,
            adx_threshold: 25.0,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(SpreadCache::new()),
            },
        );

        tokio::spawn(async move {
            analyst.run().await;
        });

        use crate::domain::trading::types::Candle;

        // Simulating a Death Cross without holding the asset
        let prices = [100.0, 100.0, 100.0, 120.0, 70.0];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "AAPL".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 100.0,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        let mut sell_detected = false;
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
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        let mut portfolio = crate::domain::trading::portfolio::Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        // Pre-load position so Sell matches verify logic
        let pos = crate::domain::trading::portfolio::Position {
            symbol: "BTC".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(100),
        };
        portfolio.positions.insert("BTC".to_string(), pos);

        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));
        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());

        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::domain::market::strategy_config::StrategyMode::Standard,
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
            commission_per_share: 0.0,
            max_position_size_pct: 0.1,
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: 0.05,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 5.0,
            min_profit_ratio: 2.0,

            macd_requires_rising: true,

            trend_tolerance_pct: 0.0,

            macd_min_threshold: 0.0,
            profit_target_multiplier: 1.5,
            adx_period: 14,
            adx_threshold: 25.0,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(SpreadCache::new()),
            },
        );

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
                volume: 100.0,
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
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        // 100k account
        let mut portfolio = crate::domain::trading::portfolio::Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));

        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());
        // Risk 2% (0.02)
        let config = AnalystConfig {
            fast_sma_period: 1,
            slow_sma_period: 2,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.02,
            strategy_mode: crate::domain::market::strategy_config::StrategyMode::Standard,
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
            commission_per_share: 0.0,
            max_position_size_pct: 0.1,
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: 0.05,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 5.0,
            min_profit_ratio: 2.0,

            macd_requires_rising: true,

            trend_tolerance_pct: 0.0,

            macd_min_threshold: 0.0,
            profit_target_multiplier: 1.5,
            adx_period: 14,
            adx_threshold: 25.0,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(SpreadCache::new()),
            },
        );

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
                volume: 100.0,
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
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        let mut portfolio = crate::domain::trading::portfolio::Portfolio::new();
        // Give explicit ETH position so Sell works
        portfolio.positions.insert(
            "ETH".to_string(),
            crate::domain::trading::portfolio::Position {
                symbol: "ETH".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::from(100),
            },
        );
        let portfolio_lock = Arc::new(RwLock::new(portfolio));

        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));
        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());

        // 2 slots
        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 2,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::domain::market::strategy_config::StrategyMode::Standard,
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 99.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.0,
            commission_per_share: 0.0,
            max_position_size_pct: 0.1,
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: 0.05,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 5.0,
            min_profit_ratio: 2.0,

            macd_requires_rising: true,

            trend_tolerance_pct: 0.0,

            macd_min_threshold: 0.0,
            profit_target_multiplier: 1.5,
            adx_period: 14,
            adx_threshold: 25.0,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(SpreadCache::new()),
            },
        );

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
                volume: 100.0,
                timestamp: i as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        // Give Analyst time to process all candles
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

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
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);
        let portfolio = Arc::new(RwLock::new(
            crate::domain::trading::portfolio::Portfolio::new(),
        ));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio,
        ));
        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());

        // Advanced mode with long trend SMA
        let config = AnalystConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            sma_threshold: 0.0,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.0,
            strategy_mode: crate::domain::market::strategy_config::StrategyMode::Advanced,
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
            commission_per_share: 0.0,
            max_position_size_pct: 0.1,
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: 0.05,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 5.0,
            min_profit_ratio: 2.0,

            macd_requires_rising: true,

            trend_tolerance_pct: 0.0,

            macd_min_threshold: 0.0,

            profit_target_multiplier: 1.5,
            adx_period: 14,
            adx_threshold: 25.0,
        };
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(SpreadCache::new()),
            },
        );

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
                volume: 100.0,
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
                volume: 100.0,
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
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        use crate::domain::trading::portfolio::Portfolio;
        // Start with empty portfolio - this is the production issue scenario
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::from(100000); // $100,000 starting cash
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));
        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());

        // Production-like configuration
        let config = AnalystConfig {
            fast_sma_period: 20,
            slow_sma_period: 60,
            max_positions: 5,
            trade_quantity: Decimal::from(1), // Fallback if risk sizing not used
            sma_threshold: 0.0005,
            order_cooldown_seconds: 0,
            risk_per_trade_percent: 0.01, // 1% of equity per trade
            strategy_mode: crate::domain::market::strategy_config::StrategyMode::Dynamic,
            trend_sma_period: 200,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.005,
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            rsi_threshold: 100.0,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            slippage_pct: 0.001,
            commission_per_share: 0.0,
            max_position_size_pct: 0.1, // 10% maximum position size
            bb_period: 20,
            bb_std_dev: 2.0,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: 0.05,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 5.0,
            min_profit_ratio: 2.0,

            macd_requires_rising: true,

            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            profit_target_multiplier: 1.5,
            adx_period: 14,
            adx_threshold: 25.0,
        };


        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            config.fast_sma_period,
            config.slow_sma_period,
            config.sma_threshold,
        ));
        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service: market_service,
                candle_repository: None,
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(SpreadCache::new()),
            },
        );

        tokio::spawn(async move {
            analyst.run().await;
        });

        // Generate a golden cross scenario
        let prices = vec![
            100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0,
            100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 102.0, 103.0, 104.0, 105.0,
            106.0, 107.0, 108.0, 109.0, 110.0, 111.0, 112.0, 113.0, 114.0, 115.0, 116.0, 117.0,
            118.0, 119.0, 120.0, 121.0, 122.0, 123.0, 124.0, 125.0, 126.0, 127.0, 128.0, 129.0,
            130.0, 131.0, 132.0, 133.0, 134.0, 135.0, 136.0, 137.0, 138.0, 139.0, 140.0, 141.0,
            142.0, 143.0, 144.0, 145.0,
        ];

        for (i, p) in prices.iter().enumerate() {
            let candle = Candle {
                symbol: "NVDA".to_string(),
                open: Decimal::from_f64_retain(*p).unwrap(),
                high: Decimal::from_f64_retain(*p).unwrap(),
                low: Decimal::from_f64_retain(*p).unwrap(),
                close: Decimal::from_f64_retain(*p).unwrap(),
                volume: 1000000.0,
                timestamp: (i * 1000) as i64,
            };
            let event = MarketEvent::Candle(candle);
            market_tx.send(event).await.unwrap();
        }

        // Should receive at least one buy signal
        let proposal =
            tokio::time::timeout(std::time::Duration::from_millis(500), proposal_rx.recv())
                .await
                .expect("Should receive a proposal within timeout")
                .expect("Should receive a buy signal");

        assert_eq!(
            proposal.side,
            OrderSide::Buy,
            "Should generate a buy signal"
        );

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
