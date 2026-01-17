use crate::application::market_data::candle_aggregator::CandleAggregator;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::cost_evaluator::CostEvaluator;
use crate::application::optimization::win_rate_provider::{StaticWinRateProvider, WinRateProvider};

use crate::application::risk_management::trailing_stops::StopState;

use crate::application::strategies::TradingStrategy;
use crate::application::strategies::strategy_selector::StrategySelector;

use crate::application::agents::trade_evaluator::{EvaluationInput, TradeEvaluator};
use crate::domain::market::market_regime::MarketRegime;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::{CandleRepository, StrategyRepository};
use crate::domain::trading::fee_model::FeeModel;
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
    pub smc_volume_multiplier: f64, // Volume multiplier for OB confirmation (e.g. 1.5x average)
    pub risk_appetite_score: Option<u8>, // Base Risk Appetite Score (1-9) for dynamic scaling
    // Breakout Strategy Configuration
    pub breakout_lookback: usize,
    pub breakout_threshold_pct: f64,
    pub breakout_volume_mult: f64,
    // Hard Stop Configuration
    pub max_loss_per_trade_pct: f64, // Maximum loss per trade before forced exit (e.g., -0.05 = -5%)
}

impl Default for AnalystConfig {
    fn default() -> Self {
        Self {
            fast_sma_period: 10,
            slow_sma_period: 20,
            max_positions: 5,
            trade_quantity: rust_decimal::Decimal::ONE,
            sma_threshold: 0.005, // Raised from 0.001 - after signal sensitivity, Risk-2 gets ~0.0025 (0.25%)
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
            smc_volume_multiplier: 1.5,
            risk_appetite_score: None,
            breakout_lookback: 10,
            breakout_threshold_pct: 0.002,
            breakout_volume_mult: 1.1,
            max_loss_per_trade_pct: -0.05, // -5% max loss per trade
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
            smc_volume_multiplier: 1.5, // Default, not yet in base Config
            risk_appetite_score: config.risk_appetite.map(|r| r.score()),
            breakout_lookback: 20, // Increased lookback for more significant levels
            breakout_threshold_pct: 0.0005, // 0.05% threshold (sensitive)
            breakout_volume_mult: 0.1, // 10% of average (effectively disable volume filter for now)
            max_loss_per_trade_pct: -0.05,
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

        // Apply signal sensitivity factor for lower risk profiles
        // This makes Conservative/Balanced profiles generate more signals
        let sensitivity = appetite.calculate_signal_sensitivity_factor();
        self.sma_threshold *= sensitivity;

        // Reduce confirmation bars for conservative profiles (1 for score <= 4, else keep)
        if appetite.score() <= 4 {
            self.signal_confirmation_bars = 1;
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
    pub spread_cache: Arc<SpreadCache>,
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

    trade_evaluator: TradeEvaluator,
    warmup_service: super::warmup_service::WarmupService,
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

        // Initialize Cost Evaluator for profit-aware trading
        let cost_evaluator = CostEvaluator::with_spread_cache(
            config.fee_model.clone(),
            config.spread_bps,
            dependencies.spread_cache.clone(),
        );

        let trade_filter =
            crate::application::trading::trade_filter::TradeFilter::new(cost_evaluator.clone());
        let trade_evaluator = TradeEvaluator::new(trade_filter);

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
            trade_evaluator,
            warmup_service,
            enabled_timeframes,
            cmd_rx,
        }
    }

    #[doc(hidden)]
    pub fn get_context(&self, symbol: &str) -> Option<&SymbolContext> {
        self.symbol_states.get(symbol)
    }

    #[doc(hidden)]
    pub fn get_context_mut(&mut self, symbol: &str) -> Option<&mut SymbolContext> {
        self.symbol_states.get_mut(symbol)
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

    async fn manage_pending_orders(
        execution_service: &std::sync::Arc<dyn ExecutionService>,
        context: &mut SymbolContext,
        symbol: &str,
        timestamp: i64,
    ) {
        if context.position_manager.check_timeout(timestamp, 60000) {
            // 60s timeout
            info!(
                "Analyst [{}]: Pending order TIMEOUT detected. Checking open orders to CANCEL...",
                symbol
            );

            // 1. Fetch Open Orders
            match execution_service.get_open_orders().await {
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
                            if let Err(e) = execution_service.cancel_order(&order.id).await {
                                error!(
                                    "Analyst [{}]: Failed to cancel order {}: {}",
                                    symbol, order.id, e
                                );
                            }
                        }
                        // Order status update will clear pending state
                    }
                }
                Err(e) => {
                    error!("Analyst [{}]: Failed to fetch open orders: {}", symbol, e);
                }
            }
        }
    }

    async fn process_candle(&mut self, candle: crate::domain::trading::types::Candle) {
        let symbol = candle.symbol.clone();
        let price = candle.close;
        let timestamp = candle.timestamp * 1000;


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

        // 3.5. Auto-initialize Trailing Stop for existing positions
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
            let entry_price = pos.average_price;
            let atr =
                rust_decimal::Decimal::from_f64_retain(context.last_features.atr.unwrap_or(1.0))
                    .unwrap_or(rust_decimal::Decimal::ONE);
            let multiplier =
                rust_decimal::Decimal::from_f64_retain(context.config.trailing_stop_atr_multiplier)
                    .unwrap_or(rust_decimal::Decimal::from(3));

            context.position_manager.trailing_stop =
                crate::application::risk_management::trailing_stops::StopState::on_buy(
                    entry_price,
                    atr,
                    multiplier,
                );

            if let Some(stop_price) = context.position_manager.trailing_stop.get_stop_price() {
                info!(
                    "Analyst [{}]: Auto-initialized trailing stop (entry={:.2}, stop={:.2}, atr={:.2})",
                    symbol, entry_price, stop_price, atr
                );
            }
        }

        // 4. Check Trailing Stop (Priority Exit) via PositionManager
        let atr_decimal =
            rust_decimal::Decimal::from_f64_retain(context.last_features.atr.unwrap_or(0.0))
                .unwrap_or(rust_decimal::Decimal::ZERO);
        let multiplier_decimal =
            rust_decimal::Decimal::from_f64_retain(context.config.trailing_stop_atr_multiplier)
                .unwrap_or(rust_decimal::Decimal::from(3));

        let mut signal = context.position_manager.check_trailing_stop(
            &symbol,
            price,
            atr_decimal,
            multiplier_decimal,
        );
        let trailing_stop_triggered = signal.is_some();

        // Check Partial Take-Profit (Swing Trading Upgrade)
        #[allow(clippy::collapsible_if)]
        if !trailing_stop_triggered && has_position {
            if let Some(proposal) = crate::application::agents::signal_processor::SignalProcessor::check_partial_take_profit(
                 context,
                 &symbol,
                 price,
                 timestamp,
                 portfolio_data.map(|p| &p.positions)
             ) {
                 info!("Analyst [{}]: Executing Partial Take-Profit...", symbol);
                  match self.proposal_tx.try_send(proposal) {
                        Ok(_) => {
                            context.taken_profit = true;
                            // Don't process further signals this tick
                            return;
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            warn!("Analyst [{}]: Proposal channel FULL.", symbol);
                            return;
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            error!("Analyst [{}]: Proposal channel CLOSED.", symbol);
                            return;
                        }
                  }
             }
        }

        // Monitor pending order timeout
        Self::manage_pending_orders(&self.execution_service, context, &symbol, timestamp).await;

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

        // 6. Post-Signal Validation & Execution Logic
        if let Some(side) = signal {
            let input = EvaluationInput {
                signal: side,
                symbol: &symbol,
                price,
                timestamp,
                regime: &regime,
                execution_service: &self.execution_service,
                has_position,
            };

            if let Some(proposal) = self
                .trade_evaluator
                .evaluate_and_propose(context, input)
                .await
            {
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
                            let atr_decimal = rust_decimal::Decimal::from_f64_retain(atr)
                                .unwrap_or(rust_decimal::Decimal::ONE);
                            let multiplier_decimal = rust_decimal::Decimal::from_f64_retain(
                                context.config.trailing_stop_atr_multiplier,
                            )
                            .unwrap_or(rust_decimal::Decimal::from(3));

                            context.position_manager.trailing_stop =
                                StopState::on_buy(price, atr_decimal, multiplier_decimal);
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
    }

    #[doc(hidden)]
    pub async fn ensure_symbol_initialized(
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
    #[doc(hidden)]
    pub async fn handle_news_signal(&mut self, signal: crate::domain::listener::NewsSignal) {
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
                                      let new_stop_f64 = price_f64 - (atr * tight_multiplier.max(0.5));
                                      let new_stop = rust_decimal::Decimal::from_f64_retain(new_stop_f64)
                                          .unwrap_or(*stop_price);
                                      if new_stop > *stop_price {
                                          *stop_price = new_stop;
                                          info!("Analyst: News TIGHTENED Trailing Stop for {} to {} (Locking Gains)", signal.symbol, new_stop);
                                      }
                                  } else {
                                      // Create new tight stop
                                      let atr_decimal = rust_decimal::Decimal::from_f64_retain(atr)
                                          .unwrap_or(rust_decimal::Decimal::ONE);
                                      let tight_mult_decimal = rust_decimal::Decimal::from_f64_retain(tight_multiplier.max(0.5))
                                          .unwrap_or(rust_decimal::Decimal::ONE);

                                      context.position_manager.trailing_stop =
                                         crate::application::risk_management::trailing_stops::StopState::on_buy(price, atr_decimal, tight_mult_decimal);
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
