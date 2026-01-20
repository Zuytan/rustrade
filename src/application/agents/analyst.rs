use crate::application::market_data::candle_aggregator::CandleAggregator;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::cost_evaluator::CostEvaluator;
use crate::application::optimization::win_rate_provider::{StaticWinRateProvider, WinRateProvider};

use crate::application::strategies::TradingStrategy;

use crate::application::agents::trade_evaluator::{EvaluationInput, TradeEvaluator};
use crate::domain::market::market_regime::MarketRegime;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::{CandleRepository, StrategyRepository};
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::OrderStatus;
use crate::domain::trading::types::{MarketEvent, OrderSide, TradeProposal};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};

use crate::domain::trading::symbol_context::SymbolContext;

pub use crate::application::agents::analyst_config::AnalystConfig;

#[derive(Debug)]
pub enum AnalystCommand {
    UpdateConfig(Box<AnalystConfig>),
    ProcessNews(crate::domain::listener::NewsSignal),
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
    ///
    /// Delegates to [`regime_handler::detect_market_regime`]
    async fn detect_market_regime(
        repo: &Option<Arc<dyn CandleRepository>>,
        symbol: &str,
        candle_timestamp: i64,
        context: &SymbolContext,
    ) -> MarketRegime {
        super::regime_handler::detect_market_regime(repo, symbol, candle_timestamp, context).await
    }

    /// Manages pending orders and handles timeouts.
    ///
    /// Delegates to [`position_lifecycle::manage_pending_orders`]
    async fn manage_pending_orders(
        execution_service: &std::sync::Arc<dyn ExecutionService>,
        context: &mut SymbolContext,
        symbol: &str,
        timestamp: i64,
    ) {
        super::position_lifecycle::manage_pending_orders(
            execution_service,
            context,
            symbol,
            timestamp,
            60000, // 60s timeout
        )
        .await
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
        super::regime_handler::apply_dynamic_risk_scaling(context, &regime, &symbol);

        // 1.6 Adaptive Strategy Switching (Phase 3)
        super::regime_handler::apply_adaptive_strategy_switching(
            context,
            &regime,
            &context.config.clone(),
            &symbol,
        );

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
                            super::position_lifecycle::initialize_trailing_stop_on_buy(
                                context, price,
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

        if price == Decimal::ZERO {
            warn!(
                "Analyst: No price data for {}. Cannot process news.",
                signal.symbol
            );
            return;
        }

        use super::news_handler::{
            NewsAction, process_bearish_news, process_bullish_news, send_news_proposal,
        };

        match signal.sentiment {
            crate::domain::listener::NewsSentiment::Bullish => {
                let action = process_bullish_news(
                    &self.config,
                    &self.execution_service,
                    &signal,
                    context,
                    price,
                    timestamp.timestamp(),
                )
                .await;

                if let NewsAction::Buy(proposal) = action
                    && let Err(e) = send_news_proposal(&self.proposal_tx, proposal).await
                {
                    error!("{}", e);
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
                    let action = process_bearish_news(
                        &signal,
                        context,
                        (pos.quantity, pos.average_price),
                        price,
                        timestamp.timestamp(),
                    );

                    match action {
                        NewsAction::PanicSell(proposal) => {
                            if let Err(e) = send_news_proposal(&self.proposal_tx, proposal).await {
                                error!("{}", e);
                            }
                        }
                        NewsAction::TightenStop => {
                            // Already handled inside process_bearish_news
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
