use crate::application::market_data::candle_aggregator::CandleAggregator;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::connection_health_service::{
    ConnectionHealthService, ConnectionStatus,
};
use crate::application::monitoring::cost_evaluator::CostEvaluator;
use crate::application::optimization::win_rate_provider::{StaticWinRateProvider, WinRateProvider};

use crate::application::strategies::TradingStrategy;

use crate::application::agents::trade_evaluator::TradeEvaluator;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::{CandleRepository, StrategyRepository};
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::OrderStatus;
use crate::domain::trading::types::{MarketEvent, TradeProposal};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, instrument, warn};

use crate::application::ml::data_collector::DataCollector;

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
    pub connection_health_service: Arc<ConnectionHealthService>,
    pub agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
}

#[allow(dead_code)] // candle_repository and trade_evaluator used indirectly by pipeline
pub struct Analyst {
    market_rx: Receiver<MarketEvent>,
    proposal_tx: Sender<TradeProposal>,
    execution_service: Arc<dyn ExecutionService>,
    default_strategy: Arc<dyn TradingStrategy>, // Fallback
    config: AnalystConfig,                      // Default config
    symbol_states: HashMap<String, SymbolContext>,
    candle_aggregator: CandleAggregator,
    win_rate_provider: Arc<dyn WinRateProvider>,

    #[allow(dead_code)] // Used indirectly by pipeline
    trade_evaluator: TradeEvaluator,
    pipeline: super::candle_pipeline::CandlePipeline,
    warmup_service: super::warmup_service::WarmupService,
    // Multi-timeframe configuration
    enabled_timeframes: Vec<crate::domain::market::timeframe::Timeframe>,
    cmd_rx: Receiver<AnalystCommand>,
    news_handler: crate::application::agents::news_handler::NewsHandler,
    ui_candle_tx: Option<broadcast::Sender<Candle>>,
    health_service: Arc<ConnectionHealthService>,
    market_data_online: bool,
    agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
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

        // Initialize SizingEngine with cost evaluator so position size accounts for estimated fees
        let sizing_engine = Arc::new(
            crate::application::risk_management::sizing_engine::SizingEngine::with_cost_evaluator(
                dependencies.spread_cache.clone(),
                cost_evaluator.clone(),
            ),
        );

        let trade_filter =
            crate::application::trading::trade_filter::TradeFilter::new(cost_evaluator.clone());
        let trade_evaluator = crate::application::agents::trade_evaluator::TradeEvaluator::new(
            trade_filter,
            crate::application::agents::signal_processor::SignalProcessor::new(
                sizing_engine.clone(),
            ), /* Clone for main analyst */
        );

        // Extract enabled timeframes from config (will be passed from system.rs)
        // For now, default to primary timeframe only to maintain backward compatibility
        let enabled_timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        // Initialize WarmupService
        let warmup_service = super::warmup_service::WarmupService::new(
            dependencies.market_service.clone(),
            dependencies.strategy_repository.clone(),
            dependencies.ui_candle_tx.clone(),
        );

        // Initialize CandlePipeline with its own instances
        let pipeline_cost_evaluator = CostEvaluator::with_spread_cache(
            config.fee_model.clone(),
            config.spread_bps,
            dependencies.spread_cache.clone(),
        );
        let pipeline_trade_filter =
            crate::application::trading::trade_filter::TradeFilter::new(pipeline_cost_evaluator);

        let pipeline_signal_processor =
            crate::application::agents::signal_processor::SignalProcessor::new(
                sizing_engine.clone(),
            );
        let pipeline_trade_evaluator =
            crate::application::agents::trade_evaluator::TradeEvaluator::new(
                pipeline_trade_filter,
                pipeline_signal_processor,
            );
        // Initialize Data Collector if enabled
        let data_collector = if config.enable_ml_data_collection {
            let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            path.push("data");
            path.push("ml");
            // Ensure directory exists
            let _ = std::fs::create_dir_all(&path);
            path.push("training_data.csv");

            info!("Analyst: ML Data Collection ENABLED. Output: {:?}", path);
            Some(Arc::new(Mutex::new(DataCollector::new(path))))
        } else {
            None
        };

        let pipeline = super::candle_pipeline::CandlePipeline::new(
            dependencies.execution_service.clone(),
            dependencies.candle_repository.clone(),
            pipeline_trade_evaluator,
            data_collector,
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
            win_rate_provider,
            trade_evaluator,
            pipeline,
            warmup_service,
            enabled_timeframes,
            cmd_rx,
            news_handler: crate::application::agents::news_handler::NewsHandler::new(
                crate::application::agents::signal_processor::SignalProcessor::new(
                    sizing_engine.clone(),
                ),
            ),
            ui_candle_tx: dependencies.ui_candle_tx,
            health_service: dependencies.connection_health_service,
            market_data_online: true, // Default to true, will be updated by run loop
            agent_registry: dependencies.agent_registry,
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

        // Subscribe to Health Events ONCE before the loop to avoid missing events.
        // Creating a new subscriber inside select! causes a race condition where
        // events broadcast between iterations are permanently lost.
        let mut health_rx = self.health_service.subscribe();

        // Sync initial state from health service to avoid stale default
        let initial_status = self.health_service.get_market_data_status().await;
        self.market_data_online = initial_status == ConnectionStatus::Online;
        info!(
            "Analyst: Initial MarketData status: {:?} (online={})",
            initial_status, self.market_data_online
        );

        // Initial Heartbeat
        self.agent_registry
            .update_heartbeat(
                "Analyst",
                crate::application::monitoring::agent_status::HealthStatus::Healthy,
            )
            .await;

        let mut health_check_interval = tokio::time::interval(std::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = health_check_interval.tick() => {
                    self.agent_registry
                        .update_heartbeat(
                            "Analyst",
                            if self.market_data_online {
                                crate::application::monitoring::agent_status::HealthStatus::Healthy
                            } else {
                                crate::application::monitoring::agent_status::HealthStatus::Degraded
                            },
                        )
                        .await;

                    self.agent_registry
                        .update_metric(
                            "Analyst",
                            "active_symbols",
                            self.symbol_states.len().to_string()
                        )
                        .await;
                }
                res = self.market_rx.recv() => {
                    match res {
                        Some(event) => {
                            match event {
                                MarketEvent::Quote {
                                    symbol,
                                    price,
                                    quantity,
                                    timestamp,
                                } => {
                                    if let Some(candle) = self.candle_aggregator.on_quote(&symbol, price, quantity, timestamp)
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

                // Handle Health Events (using persistent subscriber)
                Ok(health_event) = health_rx.recv() => {
                    if health_event.component == "MarketData" {
                         match health_event.status {
                             ConnectionStatus::Online => {
                                 if !self.market_data_online {
                                     debug!("Analyst: Market Data back ONLINE. Resuming analysis.");
                                     self.market_data_online = true;
                                 }
                             }
                             ConnectionStatus::Offline => {
                                 if self.market_data_online {
                                     debug!("Analyst: Market Data OFFLINE. Pausing analysis to prevent calculations on stale data.");
                                     self.market_data_online = false;
                                 }
                             }
                             _ => {}
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
                                 info!(
                                     order_id = %order_update.order_id,
                                     symbol = %order_update.symbol,
                                     status = ?order_update.status,
                                     "Analyst: Order resolved. Clearing pending state."
                                 );
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
    // HELPER METHODS
    // ============================================================================

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

    #[instrument(skip(self, candle), fields(symbol = %candle.symbol))]
    async fn process_candle(&mut self, candle: crate::domain::trading::types::Candle) {
        let symbol = candle.symbol.clone();
        let timestamp = candle.timestamp * 1000;

        // --- RESILIENCE GUARD ---
        if !self.market_data_online {
            debug!(
                "Analyst [{}]: Synthesis PAUSED (Market Data Offline). Skipping candle.",
                symbol
            );
            return;
        }
        // -------------------------

        // Broadcast to UI
        if let Some(tx) = &self.ui_candle_tx {
            let _ = tx.send(candle.clone());
        }

        // 1. Ensure symbol context is initialized
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

        // Reset config to default to prevent regime-based config drift
        context.config = self.config.clone();

        // 2. Get portfolio for pipeline context
        let portfolio = self.execution_service.get_portfolio().await.ok();

        // 3. Build pipeline context
        let mut pipeline_ctx = super::candle_pipeline::PipelineContext {
            symbol: &symbol,
            candle: &candle,
            context,
            portfolio: portfolio.as_ref(),
        };

        // 4. Process through pipeline (6 discrete stages)
        if let Some(proposal) = self.pipeline.process(&mut pipeline_ctx).await {
            // 5. Send proposal to risk manager
            match self.proposal_tx.try_send(proposal) {
                Ok(_) => {
                    info!("Analyst [{}]: Proposal sent to RiskManager ✓", symbol);
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

        // 6. Monitor pending order timeout
        Self::manage_pending_orders(
            &self.execution_service,
            pipeline_ctx.context,
            &symbol,
            timestamp,
        )
        .await;
    }

    #[doc(hidden)]
    #[instrument(skip(self))]
    pub async fn ensure_symbol_initialized(
        &mut self,
        symbol: &str,
        end_time: chrono::DateTime<chrono::Utc>,
    ) {
        if !self.symbol_states.contains_key(symbol) {
            info!(
                symbol = %symbol,
                warmup_end = %end_time,
                "Analyst: Initializing context"
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
    #[instrument(skip(self, signal), fields(symbol = %signal.symbol, sentiment = ?signal.sentiment))]
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

        use super::news_handler::{NewsAction, process_bearish_news, send_news_proposal};

        match signal.sentiment {
            crate::domain::listener::NewsSentiment::Bullish => {
                let action = self
                    .news_handler
                    .process_bullish_news(
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
