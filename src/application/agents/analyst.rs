use crate::application::market_data::candle_aggregator::CandleAggregator;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::cost_evaluator::CostEvaluator;
use crate::application::optimization::win_rate_provider::{StaticWinRateProvider, WinRateProvider};

use crate::application::risk_management::trailing_stops::StopState;

use crate::application::strategies::TradingStrategy;
use crate::application::strategies::strategy_selector::StrategySelector;

use crate::domain::market::market_regime::MarketRegime;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::{CandleRepository, StrategyRepository};
use crate::domain::trading::fee_model::FeeModel; // Added
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::OrderStatus; // Added
use crate::domain::trading::types::{MarketEvent, OrderSide, TradeProposal};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};

use crate::domain::trading::symbol_context::SymbolContext;

#[derive(Debug)]
pub enum AnalystCommand {
    UpdateConfig(Box<AnalystConfig>),
    ProcessNews(crate::domain::listener::NewsSignal),
}

fn default_fee_model() -> Arc<dyn FeeModel> {
    Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
        Decimal::ZERO,
        Decimal::ZERO,
    ))
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
    #[serde(skip, default = "default_fee_model")] // FeeModel is trait object
    pub fee_model: Arc<dyn FeeModel>,
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
    // SMC Strategy Configuration
    pub smc_ob_lookback: usize,          // Order Block lookback period
    pub smc_min_fvg_size_pct: f64,       // Minimum Fair Value Gap size (e.g., 0.005 = 0.5%)
    pub risk_appetite_score: Option<u8>, // Base Risk Appetite Score (1-9) for dynamic scaling
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                rust_decimal::Decimal::ZERO,
                rust_decimal::Decimal::ZERO,
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
            fee_model: config.create_fee_model(),
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
            smc_ob_lookback: config.smc_ob_lookback,
            smc_min_fvg_size_pct: config.smc_min_fvg_size_pct,
            risk_appetite_score: config.risk_appetite.map(|r| r.score()),
        }
    }
}

impl AnalystConfig {
    pub fn apply_risk_appetite(
        &mut self,
        appetite: &crate::domain::risk::risk_appetite::RiskAppetite,
    ) {
        self.risk_per_trade_percent = appetite.calculate_risk_per_trade_percent();
        self.trailing_stop_atr_multiplier = appetite.calculate_trailing_stop_multiplier();
        self.rsi_threshold = appetite.calculate_rsi_threshold();
        self.max_position_size_pct = appetite.calculate_max_position_size_pct();
        self.min_profit_ratio = appetite.calculate_min_profit_ratio();
        self.macd_requires_rising = appetite.requires_macd_rising();
        self.trend_tolerance_pct = appetite.calculate_trend_tolerance_pct();
        self.macd_min_threshold = appetite.calculate_macd_min_threshold();
        self.profit_target_multiplier = appetite.calculate_profit_target_multiplier();
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
    default_strategy: Arc<dyn TradingStrategy>, // Fallback
    config: AnalystConfig,                      // Default config
    symbol_states: HashMap<String, SymbolContext>,
    candle_aggregator: CandleAggregator,
    candle_repository: Option<Arc<dyn CandleRepository>>,
    win_rate_provider: Arc<dyn WinRateProvider>,

    trade_filter: crate::application::trading::trade_filter::TradeFilter,
    warmup_service: super::warmup_service::WarmupService, // NEW: Extracted warmup logic
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
            config.fee_model.clone(),
            config.spread_bps, // Default fallback if real spread unavailable
            dependencies.spread_cache.clone(), // Real-time spreads from WebSocket!
        );

        let trade_filter =
            crate::application::trading::trade_filter::TradeFilter::new(cost_evaluator.clone());

        // Extract enabled timeframes from config (will be passed from system.rs)
        // For now, default to primary timeframe only to maintain backward compatibility
        let enabled_timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        // Initialize WarmupService
        let warmup_service = super::warmup_service::WarmupService::new(
            dependencies.market_service.clone(),
            dependencies.strategy_repository.clone(),
            dependencies.ui_candle_tx.clone(),
        );

        Self {
            market_rx,
            proposal_tx,
            execution_service: dependencies.execution_service,
            default_strategy,
            config,
            symbol_states: HashMap::new(),
            candle_aggregator: CandleAggregator::new(
                dependencies.candle_repository.clone(),
                dependencies.spread_cache.clone(),
            ),
            candle_repository: dependencies.candle_repository,
            win_rate_provider,
            trade_filter,
            warmup_service,
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
                res = self.market_rx.recv() => {
                    match res {
                        Some(event) => {
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
                        None => {
                            info!("Analyst: Market event channel closed. Exiting main loop.");
                            break;
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
                        AnalystCommand::ProcessNews(signal) => {
                            info!("Analyst: Received News Signal for {}: {:?} - {}", signal.symbol, signal.sentiment, signal.headline);
                            // Process valid signals
                            self.handle_news_signal(signal).await;
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

    async fn process_candle(&mut self, candle: crate::domain::trading::types::Candle) {
        let symbol = candle.symbol.clone();
        let price = candle.close;
        let timestamp = candle.timestamp * 1000;
        let price_f64 = price.to_f64().unwrap_or(0.0);

        // Broadcast to UI
        // Broadcast to UI (via warmup_service which has ui_candle_tx)
        // UI broadcasting is now handled by warmup_service during warmup

        // 1. Get/Init Context (Consolidated with ensure_symbol_initialized)
        let timestamp_dt = chrono::DateTime::from_timestamp(candle.timestamp, 0)
            .unwrap_or_default()
            .with_timezone(&chrono::Utc);

        self.ensure_symbol_initialized(&symbol, timestamp_dt).await;

        let context = match self.symbol_states.get_mut(&symbol) {
            Some(ctx) => ctx,
            None => {
                warn!(
                    "Analyst [{}]: Symbol state missing after initialization. Skipping candle.",
                    symbol
                );
                return;
            }
        };

        // RESET Config to Master (Base) to clear any previous temporary dynamic overrides
        // This ensures transient regime spikes don't permanently alter the symbol's config
        context.config = self.config.clone();

        // 1.5 Detect Market Regime
        let regime =
            Self::detect_market_regime(&self.candle_repository, &symbol, candle.timestamp, context)
                .await;

        // 1.5.5 Dynamic Risk Scaling
        // AUTOMATICALLY lower risk in Volatile or Bearish regimes
        if let Some(base_score) = context.config.risk_appetite_score {
            let modifier = match regime.regime_type {
                crate::domain::market::market_regime::MarketRegimeType::Volatile => -3,
                crate::domain::market::market_regime::MarketRegimeType::TrendingDown => -2,
                _ => 0,
            };

            if modifier != 0 {
                let new_score = (base_score as i8 + modifier).clamp(1, 9) as u8;
                if let Ok(new_appetite) =
                    crate::domain::risk::risk_appetite::RiskAppetite::new(new_score)
                {
                    context.config.apply_risk_appetite(&new_appetite);
                    info!(
                        "Analyst [{}]: Dynamic Risk Scaling active. Score {} -> {} ({:?})",
                        symbol, base_score, new_score, regime.regime_type
                    );
                }
            }
        }

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

        // 3.5. Auto-initialize Trailing Stop for existing positions (P0 Fix)
        // If we have a position but no active trailing stop, initialize it
        // This handles cases where:
        // - Position existed from previous session
        // - Position was created manually
        // - Analyst restarted after Buy but before position was closed
        if has_position
            && !context.position_manager.trailing_stop.is_active()
            && let Some(portfolio) = portfolio_data
            && let Some(pos) = portfolio.positions.get(&symbol)
        {
            let entry_price = pos.average_price.to_f64().unwrap_or(price_f64);
            let atr = context.last_features.atr.unwrap_or(1.0);

            context.position_manager.trailing_stop =
                crate::application::risk_management::trailing_stops::StopState::on_buy(
                    entry_price,
                    atr,
                    context.config.trailing_stop_atr_multiplier,
                );

            if let Some(stop_price) = context.position_manager.trailing_stop.get_stop_price() {
                info!(
                    "Analyst [{}]: Auto-initialized trailing stop (entry={:.2}, stop={:.2}, atr={:.2})",
                    symbol, entry_price, stop_price, atr
                );
            }
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
        if !trailing_stop_triggered
            && has_position
            && let Some(portfolio) = portfolio_data
            && let Some(pos) = portfolio.positions.get(&symbol)
            && pos.quantity > Decimal::ZERO
        {
            let avg_price = pos.average_price.to_f64().unwrap_or(1.0);
            let pnl_pct = (price_f64 - avg_price) / avg_price;

            // Check if we hit profit target and haven't taken profit yet
            if pnl_pct >= context.config.take_profit_pct && !context.taken_profit {
                let quantity_to_sell = (pos.quantity * Decimal::new(5, 1)).round_dp(4); // 50%

                if quantity_to_sell > Decimal::ZERO {
                    info!(
                        "Analyst: Triggering Partial Take-Profit (50%) for {} at {:.2}% Gain",
                        symbol,
                        pnl_pct * 100.0
                    );

                    let proposal = TradeProposal {
                        symbol: symbol.clone(),
                        side: OrderSide::Sell,
                        price, // Use original Decimal

                        quantity: quantity_to_sell,
                        order_type: crate::domain::trading::types::OrderType::Market,
                        reason: format!("Partial Take-Profit (+{:.2}%)", pnl_pct * 100.0),
                        timestamp,
                    };

                    match self.proposal_tx.try_send(proposal) {
                        Ok(_) => {
                            context.taken_profit = true;
                            // Don't process further signals this tick
                            return;
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            warn!(
                                "Analyst [{}]: Proposal channel FULL - RiskManager slow. Backpressure applied, skipping proposal.",
                                symbol
                            );
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
                        info!(
                            "Analyst [{}]: No open orders found on exchange. Clearing local pending state.",
                            symbol
                        );
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
            signal = super::signal_processor::SignalProcessor::generate_signal(
                context,
                &symbol,
                price,
                timestamp,
                has_position,
            );

            // Apply RSI filter
            signal = super::signal_processor::SignalProcessor::apply_rsi_filter(
                signal, context, &symbol,
            );

            // Suppress sell signals when trailing stop is active
            signal = super::signal_processor::SignalProcessor::suppress_sell_if_trailing_stop(
                signal,
                context,
                &symbol,
                trailing_stop_triggered,
            );
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

            let mut proposal = match super::signal_processor::SignalProcessor::build_proposal(
                &self.config,
                &self.execution_service,
                symbol.clone(),
                side,
                price,
                timestamp,
                reason,
            )
            .await
            {
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
                Decimal::from_f64_retain(expectancy.expected_value).unwrap_or(Decimal::ZERO)
                    * proposal.quantity
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
                        && atr > 0.0
                    {
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
            let (strategy, config) = self
                .warmup_service
                .resolve_strategy(symbol, self.default_strategy.clone(), &self.config)
                .await;

            let mut context = SymbolContext::new(
                config,
                strategy,
                self.win_rate_provider.clone(),
                self.enabled_timeframes.clone(),
            );

            // WARMUP: Fetch historical data to initialize indicators
            self.warmup_service
                .warmup_context(&mut context, symbol, end_time)
                .await;

            self.symbol_states.insert(symbol.to_string(), context);
        }
    }
    async fn handle_news_signal(&mut self, signal: crate::domain::listener::NewsSignal) {
        // Ensure context exists
        let timestamp = chrono::Utc::now();
        self.ensure_symbol_initialized(&signal.symbol, timestamp)
            .await;

        let context = match self.symbol_states.get_mut(&signal.symbol) {
            Some(ctx) => ctx,
            None => {
                warn!(
                    "Analyst: Could not initialize context for {}. Ignoring news.",
                    signal.symbol
                );
                return;
            }
        };

        // Get latest price
        let price = context
            .candle_history
            .back()
            .map(|c| c.close)
            .unwrap_or(Decimal::ZERO);
        let price_f64 = price.to_f64().unwrap_or(0.0);

        if price == Decimal::ZERO {
            warn!(
                "Analyst: No price data for {}. Cannot process news.",
                signal.symbol
            );
            return;
        }

        match signal.sentiment {
            crate::domain::listener::NewsSentiment::Bullish => {
                // Feature Extraction for Intelligence
                let sma_50 = context.last_features.sma_50.unwrap_or(0.0);
                let rsi = context.last_features.rsi.unwrap_or(50.0);

                info!(
                    "Analyst: Analyzing BULLISH news for {}. Price: {}, SMA50: {}, RSI: {}",
                    signal.symbol, price_f64, sma_50, rsi
                );

                // 1. Trend Filter: Avoid buying falling knives
                if price_f64 < sma_50 {
                    warn!(
                        "Analyst: REJECTED Bullish News for {}. Price below SMA50 (Bearish Trend).",
                        signal.symbol
                    );
                    return;
                }

                // 2. Overbought Filter: Avoid FOMO
                if rsi > 75.0 {
                    warn!(
                        "Analyst: REJECTED Bullish News for {}. RSI {} indicates Overbought.",
                        signal.symbol, rsi
                    );
                    return;
                }

                // 3. Construct Proposal
                let reason = format!("News (Trend Correct & RSI OK): {}", signal.headline);
                if let Some(mut proposal) =
                    super::signal_processor::SignalProcessor::build_proposal(
                        &self.config,
                        &self.execution_service,
                        signal.symbol.clone(),
                        OrderSide::Buy,
                        price,
                        timestamp.timestamp() * 1000,
                        reason,
                    )
                    .await
                {
                    proposal.order_type = crate::domain::trading::types::OrderType::Market;
                    info!(
                        "Analyst: Proposing BUY based on Validated News: {}",
                        signal.headline
                    );
                    if let Err(e) = self.proposal_tx.send(proposal).await {
                        error!("Failed to send news proposal: {}", e);
                    }
                }
            }
            crate::domain::listener::NewsSentiment::Bearish => {
                // Check if we hold it.
                if let Ok(portfolio) = self.execution_service.get_portfolio().await
                    && let Some(pos) = portfolio
                        .positions
                        .get(&signal.symbol)
                        .filter(|p| p.quantity > Decimal::ZERO)
                {
                    let avg_price = pos.average_price.to_f64().unwrap_or(price_f64);
                    let pnl_pct = (price_f64 - avg_price) / avg_price;

                    info!(
                        "Analyst: Processing BEARISH news for {}. PnL: {:.2}%",
                        signal.symbol,
                        pnl_pct * 100.0
                    );

                    if pnl_pct > 0.05 {
                        // SCENARIO 1: Profitable Position -> Tighten Stop to Protect Gains
                        // Tighten to 0.5% below current price
                        let atr = context.last_features.atr.unwrap_or(price_f64 * 0.01);
                        // 0.5% gap approx
                        let tight_multiplier = (price_f64 * 0.005) / atr;

                        // Manually update StopState
                        if let crate::application::risk_management::trailing_stops::StopState::ActiveStop { stop_price, .. } = &mut context.position_manager.trailing_stop {
                                     let new_stop = price_f64 - (atr * tight_multiplier.max(0.5)); // Ensure valid mult
                                     if new_stop > *stop_price {
                                         *stop_price = new_stop;
                                         info!("Analyst: News TIGHTENED Trailing Stop for {} to {:.2} (Locking Gains)", signal.symbol, new_stop);
                                     }
                                 } else {
                                     // Create new tight stop
                                     context.position_manager.trailing_stop =
                                        crate::application::risk_management::trailing_stops::StopState::on_buy(price_f64, atr, tight_multiplier.max(0.5));
                                     info!("Analyst: News CREATED Tight Trailing Stop for {}", signal.symbol);
                                 }
                    } else {
                        // SCENARIO 2: Losing or Flat Position -> Panic Sell / Damage Control
                        info!(
                            "Analyst: News Triggering PANIC SELL for {} to limit potential loss.",
                            signal.symbol
                        );

                        let proposal = TradeProposal {
                            symbol: signal.symbol.clone(),
                            side: OrderSide::Sell,
                            price: Decimal::ZERO,
                            quantity: pos.quantity, // Sell ALL
                            order_type: crate::domain::trading::types::OrderType::Market,
                            reason: format!(
                                "News Panic Sell (PnL: {:.2}%): {}",
                                pnl_pct * 100.0,
                                signal.headline
                            ),
                            timestamp: timestamp.timestamp(),
                        };

                        if let Err(e) = self.proposal_tx.send(proposal).await {
                            error!("Failed to send panic sell proposal: {}", e);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::Candle;
    use rust_decimal::prelude::FromPrimitive;
    use std::sync::Once;
    use tokio::sync::RwLock;
    use tokio::sync::mpsc;

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
                market_service,
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                Decimal::ZERO,
                Decimal::ZERO,
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
                market_service,
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                Decimal::ZERO,
                Decimal::ZERO,
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
                market_service,
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
        #[allow(clippy::collapsible_if)]
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                Decimal::ZERO,
                Decimal::ZERO,
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
                market_service,
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                Decimal::ZERO,
                Decimal::ZERO,
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
                market_service,
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                Decimal::ZERO,
                Decimal::ZERO,
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
                market_service,
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                Decimal::ZERO,
                Decimal::ZERO,
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
                market_service,
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
            fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(
                Decimal::ZERO,
                Decimal::from_f64(0.001).unwrap(),
            )),
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
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            risk_appetite_score: None,
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
                market_service,
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

    #[tokio::test]
    async fn test_news_intelligence_filters() {
        setup_logging();
        let (_market_tx, _market_rx) = mpsc::channel(10);
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        let mut portfolio = crate::domain::trading::portfolio::Portfolio::new();
        portfolio.cash = Decimal::from(100000);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));
        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());

        let config = AnalystConfig::default();
        let strategy = Arc::new(crate::application::strategies::DualSMAStrategy::new(
            10, 20, 0.0,
        ));

        let deps = AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
        };

        let mut analyst = Analyst::new(_market_rx, cmd_rx, proposal_tx, config, strategy, deps);

        analyst
            .ensure_symbol_initialized("BTC/USD", chrono::Utc::now())
            .await;

        {
            let context = analyst.symbol_states.get_mut("BTC/USD").unwrap();
            // Scenario 1: Bullish OK (Price > SMA)
            context.last_features.sma_50 = Some(40000.0);
            context.last_features.rsi = Some(50.0);
            let candle = Candle {
                symbol: "BTC/USD".to_string(),
                open: Decimal::from(50000),
                high: Decimal::from(50000),
                low: Decimal::from(50000),
                close: Decimal::from(50000),
                volume: 100.0,
                timestamp: 1000,
            };
            context.candle_history.push_back(candle);
        }

        // Send Bullish Signal
        let signal = crate::domain::listener::NewsSignal {
            symbol: "BTC/USD".to_string(),
            sentiment: crate::domain::listener::NewsSentiment::Bullish,
            headline: "Moon".to_string(),
            source: "Twitter".to_string(),
            url: Some("".to_string()),
        };

        analyst.handle_news_signal(signal.clone()).await;

        let proposal = proposal_rx
            .try_recv()
            .expect("Should have generated proposal for Bullish+Technical OK");
        assert_eq!(proposal.side, OrderSide::Buy);

        {
            let context = analyst.symbol_states.get_mut("BTC/USD").unwrap();
            // Scenario 2: Bullish REJECTED (Price < SMA)
            context.last_features.sma_50 = Some(40000.0);
            context.candle_history.back_mut().unwrap().close = Decimal::from(30000);
        }

        analyst.handle_news_signal(signal.clone()).await;
        assert!(
            proposal_rx.try_recv().is_err(),
            "Should NOT generate proposal in bearish trend"
        );

        {
            let context = analyst.symbol_states.get_mut("BTC/USD").unwrap();
            // Scenario 3: Bullish REJECTED (RSI > 75)
            context.last_features.sma_50 = Some(20000.0);
            context.last_features.rsi = Some(80.0);
            context.candle_history.back_mut().unwrap().close = Decimal::from(30000);
        }
        analyst.handle_news_signal(signal.clone()).await;
        assert!(
            proposal_rx.try_recv().is_err(),
            "Should NOT generate proposal when RSI > 75"
        );
    }

    #[tokio::test]
    async fn test_trailing_stop_suppresses_sell_signal() {
        setup_logging();
        let (market_tx, market_rx) = mpsc::channel(10);
        let (_cmd_tx, cmd_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        let mut portfolio = crate::domain::trading::portfolio::Portfolio::new();
        portfolio.cash = Decimal::from(100000);
        // Add position with existing trailing stop
        let pos = crate::domain::trading::portfolio::Position {
            symbol: "AAPL".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(150),
        };
        portfolio.positions.insert("AAPL".to_string(), pos);

        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        let exec_service = Arc::new(crate::infrastructure::mock::MockExecutionService::new(
            portfolio_lock,
        ));
        let market_service = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());

        // Config with trailing stop enabled
        let config = AnalystConfig {
            trailing_stop_atr_multiplier: 3.0,
            atr_period: 14,
            order_cooldown_seconds: 0,
            ..AnalystConfig::default()
        };

        // Custom strategy that always sells
        struct AlwaysSellStrategy;
        impl crate::application::strategies::TradingStrategy for AlwaysSellStrategy {
            fn name(&self) -> &str {
                "AlwaysSell"
            }
            fn analyze(
                &self,
                _ctx: &crate::application::strategies::AnalysisContext,
            ) -> Option<crate::application::strategies::Signal> {
                Some(crate::application::strategies::Signal::sell("Force Sell"))
            }
        }

        let strategy = Arc::new(AlwaysSellStrategy);

        let mut analyst = Analyst::new(
            market_rx,
            cmd_rx,
            proposal_tx,
            config,
            strategy,
            AnalystDependencies {
                execution_service: exec_service,
                market_service,
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

        // 1. Manually set a trailing stop in the analyst's context
        // Since we can't access internals directly, we rely on auto-initialization.
        // Analyst auto-initializes trailing stop if we have a position and config.trailing_stop_atr_multiplier > 0
        // We set config to have multiplier 3.0.
        // On the first candle, Analyst should see the position, see no T-Stop, and init it.
        // However, auto-init might happen AFTER signal processing?
        // Let's check:
        // 3.5 Auto-init Trailing Stop
        // 4. Check Trailing Stop
        // 5. Generate Signal

        // So on the FIRST candle:
        // - Auto-init happens. T-Stop is now active (set to entry price - 3*ATR).
        // - Check T-Stop matches price? If price is 150 (entry), stop is below. Not triggered.
        // - Generate Signal -> AlwaysSell says SELL.
        // - Suppress logic -> Checks if T-Stop is active. It IS active (just initialized).
        // - Result: Suppressed.

        // So just sending one candle equal to entry price should work.

        use crate::domain::trading::types::Candle;
        let candle = Candle {
            symbol: "AAPL".to_string(),
            open: Decimal::from(150),
            high: Decimal::from(150),
            low: Decimal::from(150),
            close: Decimal::from(150),
            volume: 100.0,
            timestamp: 1000,
        };

        market_tx.send(MarketEvent::Candle(candle)).await.unwrap();

        // 2. Expect NO Proposal
        // We wait a bit. If we get a proposal, it's a failure.
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(200), proposal_rx.recv()).await;

        match result {
            Ok(Some(p)) => {
                panic!(
                    "Received unexpected proposal: {:?}. Sell signal should have been suppressed by Trailing Stop!",
                    p
                );
            }
            Ok(None) => {} // Channel closed
            Err(_) => {
                // Timeout = Success (No proposal received)
                println!("✅ Sell signal successfully suppressed by active Trailing Stop.");
            }
        }
    }
}
