use crate::application::risk_management::circuit_breaker_service::{
    CircuitBreakerConfig as ServiceCircuitBreakerConfig, CircuitBreakerService,
}; // Added and Aliased
use crate::application::risk_management::liquidation_service::LiquidationService;
use crate::application::risk_management::order_reconciler::{OrderReconciler, PendingOrder}; // Added PendingOrder import
use crate::application::risk_management::pipeline::validation_pipeline::RiskValidationPipeline;
use crate::application::risk_management::portfolio_valuation_service::PortfolioValuationService;
use crate::application::risk_management::session_manager::SessionManager;
use crate::application::risk_management::state::pending_orders_tracker::PendingOrdersTracker;
use crate::application::risk_management::state::risk_state_manager::RiskStateManager;
use crate::domain::ports::{ExecutionService, MarketDataService, OrderUpdate};
use crate::domain::repositories::{CandleRepository, RiskStateRepository};
use crate::domain::risk::filters::{
    RiskValidator, ValidationContext, ValidationResult,
    buying_power_validator::{BuyingPowerConfig, BuyingPowerValidator},
    circuit_breaker_validator::{CircuitBreakerConfig, CircuitBreakerValidator},
    correlation_filter::CorrelationFilter,
    pdt_validator::{PdtConfig, PdtValidator},
    position_size_validator::{PositionSizeConfig, PositionSizeValidator},
    price_anomaly_validator::{PriceAnomalyConfig, PriceAnomalyValidator},
    sector_exposure_validator::{SectorExposureConfig, SectorExposureValidator},
    sentiment_validator::{SentimentConfig, SentimentValidator},
};
use crate::domain::risk::state::RiskState;
use crate::domain::risk::volatility_manager::VolatilityManager; // Added
use crate::domain::sentiment::Sentiment;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Order, OrderSide, TradeProposal};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock; // Added
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::application::monitoring::correlation_service::CorrelationService;
use crate::application::monitoring::performance_monitoring_service::PerformanceMonitoringService;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::commands::RiskCommand;
use crate::config::AssetClass;

pub use crate::domain::risk::risk_config::{RiskConfig, RiskConfigError};

pub struct RiskManager {
    proposal_rx: Receiver<TradeProposal>,
    external_cmd_rx: Receiver<RiskCommand>,
    order_tx: Sender<Order>,
    execution_service: Arc<dyn ExecutionService>,
    market_service: Arc<dyn MarketDataService>,
    portfolio_state_manager: Arc<PortfolioStateManager>,
    performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    correlation_service: Option<Arc<CorrelationService>>,
    risk_config: RiskConfig,
    volatility_manager: Arc<RwLock<VolatilityManager>>,

    asset_class: AssetClass,

    // NEW Architecture Components
    validation_pipeline: RiskValidationPipeline,
    #[allow(dead_code)]
    state_manager: RiskStateManager,
    #[allow(dead_code)]
    pending_orders_tracker: PendingOrdersTracker,

    // Extracted Services
    session_manager: SessionManager,
    portfolio_valuation_service: PortfolioValuationService,
    liquidation_service: LiquidationService,
    circuit_breaker_service: CircuitBreakerService, // New
    order_reconciler: OrderReconciler,              // New

    // Legacy State (Deprecated/To be removed)
    // Kept briefly if needed for transition, but aim to remove usage
    risk_state: RiskState, // REPLACED BY state_manager
    // pending_orders removed - replaced by order_reconciler

    // Runtime flags
    // halted moved to CircuitBreakerService
    daily_pnl: Decimal,

    // Cache
    current_prices: HashMap<String, Decimal>,
    // pending_reservations moved to OrderReconciler
    current_sentiment: Option<Sentiment>,
    risk_state_repository: Option<Arc<dyn RiskStateRepository>>,
    candle_repository: Option<Arc<dyn CandleRepository>>,

    // Services
    #[allow(dead_code)] // Todo: Use in LiquidationService in next task
    spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
}

impl RiskManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proposal_rx: Receiver<TradeProposal>,
        external_cmd_rx: Receiver<RiskCommand>,
        order_tx: Sender<Order>,
        execution_service: Arc<dyn ExecutionService>,
        market_service: Arc<dyn MarketDataService>,
        portfolio_state_manager: Arc<PortfolioStateManager>,
        non_pdt_mode: bool,
        asset_class: AssetClass,
        risk_config: RiskConfig,
        performance_monitor: Option<Arc<PerformanceMonitoringService>>,
        correlation_service: Option<Arc<CorrelationService>>,
        risk_state_repository: Option<Arc<dyn RiskStateRepository>>,
        candle_repository: Option<Arc<dyn CandleRepository>>,
        spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    ) -> Result<Self, RiskConfigError> {
        // Validate configuration
        risk_config
            .validate()
            .map_err(RiskConfigError::ValidationError)?;

        // --- Build Validation Pipeline ---
        let validators: Vec<Box<dyn RiskValidator>> = vec![
            // 1. Top Priority: Circuit Breaker
            Box::new(CircuitBreakerValidator::new(CircuitBreakerConfig {
                max_daily_loss_pct: risk_config.max_daily_loss_pct,
                max_drawdown_pct: risk_config.max_drawdown_pct,
                consecutive_loss_limit: risk_config.consecutive_loss_limit,
            })),
            // 2. Price Anomaly Detection (Fat Finger Protection)
            Box::new(PriceAnomalyValidator::new(PriceAnomalyConfig::default())),
            // 3. Regulatory: PDT
            Box::new(PdtValidator::new(PdtConfig {
                enabled: !non_pdt_mode && !risk_config.allow_pdt_risk,
                asset_class,
                ..Default::default()
            })),
            // 4. Diversification: Sector Exposure
            Box::new(SectorExposureValidator::new(SectorExposureConfig {
                max_sector_exposure_pct: risk_config.max_sector_exposure_pct,
                sector_provider: risk_config.sector_provider.clone(),
            })),
            // 5. Diversification: Correlation
            Box::new(CorrelationFilter::new(
                risk_config.correlation_config.clone(),
            )),
            // 6. Risk Sizing: Position Size
            Box::new(PositionSizeValidator::new(PositionSizeConfig {
                max_position_size_pct: risk_config.max_position_size_pct,
            })),
            // 7. Optimization: Sentiment
            Box::new(SentimentValidator::new(SentimentConfig::default())),
            // 8. Affordability: Buying Power (Available Cash)
            Box::new(BuyingPowerValidator::new(BuyingPowerConfig::default())),
        ];

        let validation_pipeline = RiskValidationPipeline::new(validators);

        // --- State Management ---
        let state_manager = RiskStateManager::new(
            risk_state_repository.clone(),
            Decimal::ZERO, // Initialized later in initialize_session
        );

        let pending_orders_tracker = PendingOrdersTracker::new();
        let volatility_manager = Arc::new(RwLock::new(VolatilityManager::new(
            risk_config.volatility_config.clone(),
        )));

        // Initialize extracted services
        let session_manager =
            SessionManager::new(risk_state_repository.clone(), market_service.clone());

        let portfolio_valuation_service = PortfolioValuationService::new(
            market_service.clone(),
            portfolio_state_manager.clone(),
            volatility_manager.clone(),
            asset_class,
        );

        let liquidation_service =
            LiquidationService::new(order_tx.clone(), portfolio_state_manager.clone());

        Ok(Self {
            proposal_rx,
            external_cmd_rx,
            order_tx,
            execution_service,
            market_service,
            portfolio_state_manager,

            asset_class,

            volatility_manager,

            // New Components
            validation_pipeline,
            state_manager,
            pending_orders_tracker,
            risk_config: risk_config.clone(), // Fix move error

            // Extracted Services
            session_manager,
            portfolio_valuation_service,
            liquidation_service,
            circuit_breaker_service: CircuitBreakerService::new(ServiceCircuitBreakerConfig {
                max_daily_loss_pct: risk_config.max_daily_loss_pct,
                max_drawdown_pct: risk_config.max_drawdown_pct,
                consecutive_loss_limit: risk_config.consecutive_loss_limit,
            }),
            order_reconciler: OrderReconciler::new(risk_config.pending_order_ttl_ms),

            // Legacy State (Initialized to defaults, will be synced or ignored)
            risk_state: RiskState::default(),
            // pending_orders removed
            current_prices: HashMap::new(),
            performance_monitor,
            correlation_service,

            // halted removed
            daily_pnl: Decimal::ZERO,

            // pending_reservations removed
            current_sentiment: None,
            risk_state_repository,
            candle_repository,
            spread_cache,
        })
    }

    /// Persist current risk state to database
    async fn persist_state(&self) {
        if let Some(repo) = &self.risk_state_repository {
            let state = RiskState {
                id: "global".to_string(), // Singleton for now
                session_start_equity: self.risk_state.session_start_equity,
                daily_start_equity: self.risk_state.daily_start_equity,
                equity_high_water_mark: self.risk_state.equity_high_water_mark,
                consecutive_losses: self.risk_state.consecutive_losses,
                reference_date: self.risk_state.reference_date,
                updated_at: Utc::now().timestamp(),
                daily_drawdown_reset: false, // Default or track it
            };

            if let Err(e) = repo.save(&state).await {
                error!("RiskManager: Failed to persist risk state: {}", e);
            } else {
                debug!("RiskManager: Risk state persisted successfully.");
            }
        }
    }

    /// Initialize session tracking with starting equity
    /// Delegates to SessionManager for session lifecycle management
    pub async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Get portfolio snapshot
        let snapshot = self.portfolio_state_manager.refresh().await?;

        // Delegate session initialization to SessionManager
        let risk_state = self
            .session_manager
            .initialize_session(&snapshot.portfolio, &mut self.current_prices)
            .await?;

        // Sync to legacy state (will be removed in future)
        self.risk_state = risk_state;

        info!(
            "RiskManager: Session initialized. Equity: {}, Daily Start: {}, HWM: {}",
            self.risk_state.session_start_equity,
            self.risk_state.daily_start_equity,
            self.risk_state.equity_high_water_mark
        );

        // SYNC Fix: Ensure state_manager is also updated with the initialized state
        // otherwise update_portfolio_valuation will overwrite self.risk_state with empty state
        *self.state_manager.get_state_mut() = self.risk_state.clone();

        Ok(())
    }

    /// Check if circuit breaker should trigger
    fn check_circuit_breaker(&self, current_equity: Decimal) -> Option<String> {
        self.circuit_breaker_service
            .check_circuit_breaker(&self.risk_state, current_equity)
    }

    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    fn handle_order_update(&mut self, update: OrderUpdate) -> bool {
        self.order_reconciler.handle_order_update(
            &update,
            &mut self.risk_state,
            &self.portfolio_state_manager,
        )
    }

    /// Fetch latest prices for all held positions and update valuation
    /// Delegates to PortfolioValuationService for valuation updates
    pub async fn update_portfolio_valuation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Delegate valuation to PortfolioValuationService
        let (_portfolio, current_equity) = self
            .portfolio_valuation_service
            .update_portfolio_valuation(&mut self.current_prices)
            .await?;

        // Update volatility
        let _ = self.portfolio_valuation_service.update_volatility().await;

        // Update High Water Mark via State Manager
        self.state_manager.update(current_equity, Utc::now()).await;
        // Sync legacy copy
        self.risk_state = self.state_manager.get_state().clone();

        // Check Risks (Async check)
        // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
        // Check Risks (Async check)
        // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
        if !self.circuit_breaker_service.is_halted()
            && let Some(reason) = self.check_circuit_breaker(current_equity)
        {
            tracing::error!("RiskManager MONITOR: CIRCUIT BREAKER TRIGGERED: {}", reason);
            self.circuit_breaker_service.set_halted(true);
            self.liquidate_portfolio(&reason).await;
        }

        // Capture performance snapshot if monitor available
        if let Some(monitor) = &self.performance_monitor {
            for sym in self.current_prices.keys() {
                let _ = monitor.capture_snapshot(sym).await;
            }
        }

        Ok(())
    }

    /// Update volatility manager with latest ATR/Benchmark data
    pub async fn update_volatility(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Choose benchmark symbol based on asset class
        let benchmark = match self.asset_class {
            AssetClass::Crypto => "BTC/USDT",
            _ => "SPY",
        };

        // Fetch last 20 daily candles for ATR
        let now = Utc::now();
        let start = now - chrono::Duration::days(30); // 30 days to get enough candles

        match self
            .market_service
            .get_historical_bars(benchmark, start, now, "1D")
            .await
        {
            Ok(candles) => {
                if candles.len() < 2 {
                    return Ok(());
                }

                // Calculate a simple True Range for the latest candle
                // TR = Max(H-L, H-Cp, L-Cp)
                // For simplicity here, we take H-L
                let last = &candles[candles.len() - 1];
                let high = last.high.to_f64().unwrap_or(0.0);
                let low = last.low.to_f64().unwrap_or(0.0);
                let range = high - low;

                if range > 0.0 {
                    let mut vm = self.volatility_manager.write().await;
                    vm.update(range);
                    debug!(
                        "RiskManager: Volatility updated for {}. Latest range: {:.2}, Avg: {:.2}",
                        benchmark,
                        range,
                        vm.get_average_volatility()
                    );
                }
            }
            Err(e) => {
                warn!("RiskManager: Failed to fetch volatility data: {}", e);
            }
        }

        Ok(())
    }

    /// Emergency liquidation of entire portfolio
    /// Delegates to LiquidationService for emergency liquidation logic
    async fn liquidate_portfolio(&mut self, reason: &str) {
        // Delegate liquidation to LiquidationService
        self.liquidation_service
            .liquidate_portfolio(reason, &self.current_prices)
            .await;
    }

    /// Check if we need to reset session stats (for 24/7 Crypto markets)
    fn check_daily_reset(&mut self, current_equity: Decimal) -> bool {
        // Delegate to RiskStateManager
        self.state_manager.check_daily_reset(current_equity);

        // Sync local legacy state
        let old_reset = self.risk_state.daily_drawdown_reset;
        self.risk_state = self.state_manager.get_state().clone();

        if self.risk_state.daily_drawdown_reset && !old_reset {
            self.daily_pnl = Decimal::ZERO;
            return true;
        }

        // If reference date changed, it was a reset
        let today = Utc::now().date_naive();
        if self.asset_class == AssetClass::Crypto && self.risk_state.reference_date == today {
            // It might have been updated by manager just now.
            // But we need to know if it CHANGED just now.
            // We can assume state_manager handles it.
            // Returning false is safe if manager handles persistence (via update called elsewhere).
            // But caller expects bool to call persist.

            // Simplification: always return false here and rely on update_portfolio_valuation to persist state changes?
            // But daily reset happens on proposal too.

            // Check if updated_at is recent?
            if self.risk_state.updated_at >= Utc::now().timestamp() - 1 {
                return true;
            }
        }
        false
    }

    /// Cleanup tentative filled orders and release reservations
    fn reconcile_pending_orders(&mut self, portfolio: &Portfolio) {
        self.order_reconciler
            .reconcile_pending_orders(portfolio, &self.portfolio_state_manager);
    }

    pub fn is_halted(&self) -> bool {
        self.circuit_breaker_service.is_halted()
    }

    // ============================================================================
    // COMMAND PATTERN HANDLERS
    // ============================================================================

    /// Dispatch command to appropriate handler
    async fn handle_command(&mut self, cmd: RiskCommand) -> Result<(), Box<dyn std::error::Error>> {
        match cmd {
            RiskCommand::OrderUpdate(update) => self.cmd_handle_order_update(update).await,
            RiskCommand::ValuationTick => self.cmd_handle_valuation().await,
            RiskCommand::RefreshPortfolio => self.cmd_handle_refresh().await,
            RiskCommand::ProcessProposal(proposal) => self.cmd_handle_proposal(proposal).await,
            RiskCommand::UpdateSentiment(sentiment) => {
                self.cmd_handle_update_sentiment(sentiment).await
            }
            RiskCommand::UpdateConfig(config) => self.cmd_handle_update_config(config).await,
            RiskCommand::CircuitBreakerTrigger => {
                warn!(
                    "RiskManager: MANUAL CIRCUIT BREAKER TRIGGERED! Executing Panic Liquidation."
                );
                self.liquidate_portfolio("Manual Circuit Breaker Trigger")
                    .await;
                Ok(())
            }
        }
    }

    async fn cmd_handle_update_config(
        &mut self,
        config: Box<RiskConfig>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("RiskManager: Updating risk configuration: {:?}", config);
        self.risk_config = *config;
        Ok(())
    }

    async fn cmd_handle_update_sentiment(
        &mut self,
        sentiment: Sentiment,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "RiskManager: Received Market Sentiment: {} ({})",
            sentiment.value, sentiment.classification
        );
        self.current_sentiment = Some(sentiment);
        Ok(())
    }

    /// Handle portfolio refresh command
    async fn cmd_handle_refresh(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.portfolio_state_manager.refresh().await?;
        Ok(())
    }

    /// Handle order update command
    async fn cmd_handle_order_update(
        &mut self,
        update: OrderUpdate,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.handle_order_update(update) {
            self.persist_state().await;
        }
        Ok(())
    }

    /// Handle valuation tick command
    async fn cmd_handle_valuation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.update_portfolio_valuation().await?;

        if !self.circuit_breaker_service.is_halted() {
            let snapshot = self.portfolio_state_manager.get_snapshot().await;
            if self.check_daily_reset(snapshot.portfolio.total_equity(&self.current_prices)) {
                self.persist_state().await;
            }
            self.reconcile_pending_orders(&snapshot.portfolio);
        }

        Ok(())
    }

    /// Handle trade proposal command
    async fn cmd_handle_proposal(
        &mut self,
        proposal: TradeProposal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.circuit_breaker_service.is_halted() {
            warn!(
                "RiskManager: Trading HALTED. Rejecting proposal for {}",
                proposal.symbol
            );
            return Ok(());
        }

        debug!("RiskManager: reviewing proposal {:?}", proposal);

        // Update current price
        self.current_prices
            .insert(proposal.symbol.clone(), proposal.price);

        // Get portfolio snapshot
        let mut snapshot = self.portfolio_state_manager.get_snapshot().await;

        // Refresh if stale
        if self.portfolio_state_manager.is_stale(&snapshot) {
            snapshot = self.portfolio_state_manager.refresh().await?;
        }

        // Reconcile pending orders
        self.reconcile_pending_orders(&snapshot.portfolio);

        // Calculate current equity
        let current_equity = snapshot.portfolio.total_equity(&self.current_prices);

        // Update high water mark
        if current_equity > self.risk_state.equity_high_water_mark {
            self.risk_state.equity_high_water_mark = current_equity;
        }

        // Check daily reset
        if self.check_daily_reset(current_equity) {
            self.persist_state().await;
        }

        // Circuit breaker check (Trigger Liquidation logic)
        // Keeps the system safe by checking global health before processing specific trade rules
        if let Some(reason) = self.check_circuit_breaker(current_equity) {
            error!("RiskManager: CIRCUIT BREAKER TRIGGERED - {}", reason);
            self.circuit_breaker_service.set_halted(true);
            self.liquidate_portfolio(&reason).await;
            return Ok(());
        }

        // Prepare Validation Context
        let correlation_matrix = if let Some(service) = &self.correlation_service {
            // Pre-fetch correlation matrix if service available
            // Optimization: We could let the validator ask for it, but context is passive.
            // We get existing symbols + proposal symbol
            let mut symbols: Vec<String> = snapshot.portfolio.positions.keys().cloned().collect();
            if !symbols.contains(&proposal.symbol) {
                symbols.push(proposal.symbol.clone());
            }
            service.calculate_correlation_matrix(&symbols).await.ok()
        } else {
            None
        };

        let volatility_multiplier = {
            let vm = self.volatility_manager.read().await;
            // For now we use the average multiplier if no specific current vol is fed
            // Or we could have a "get_current_multiplier" that uses a default or last known.
            // Let's assume we want to pass a value here.
            // If we don't have current volatility data, we use 1.0.
            Some(vm.calculate_multiplier(vm.get_average_volatility()))
        };

        let pending_exposure = self
            .order_reconciler
            .get_pending_exposure(&proposal.symbol, OrderSide::Buy);

        let recent_candles = if let Some(repo) = &self.candle_repository {
            // Fetch last 20 recent candles for price anomaly validation
            // We use a safe lookback window (e.g. 5 min * 20 = 100 min)
            let now_ts = Utc::now().timestamp();
            // Assumed get_range handles sort order
            repo.get_range(&proposal.symbol, now_ts - 7200, now_ts)
                .await
                .ok()
        } else {
            None
        };
        // NOTE: We pass a reference to the vector if it exists.
        // Since `recent_candles` is owned here, we need to be careful with lifetimes.
        // ValidationContext expects `Option<&'a [Candle]>`.

        let candles_ref = recent_candles.as_deref();

        let ctx = ValidationContext::new(
            &proposal,
            &snapshot.portfolio,
            current_equity,
            &self.current_prices,
            &self.risk_state,
            self.current_sentiment.as_ref(),
            correlation_matrix.as_ref(), // Pass pre-calculated matrix
            volatility_multiplier,
            pending_exposure,
            snapshot.available_cash(),
            candles_ref, // Pass recent candles from CandleRepository for PriceAnomalyValidator
        );

        // Execute Pipeline
        match self.validation_pipeline.validate(&ctx).await {
            ValidationResult::Approve => {
                // All checks passed
                self.execute_proposal_internal(proposal, &snapshot.portfolio)
                    .await?;
            }
            ValidationResult::Reject(reason) => {
                warn!(
                    "RiskManager: Rejecting {:?} order for {} - {}",
                    proposal.side, proposal.symbol, reason
                );
            }
        }

        Ok(())
    }

    /// Internal proposal execution logic (extracted from run())
    async fn execute_proposal_internal(
        &mut self,
        proposal: TradeProposal,
        _portfolio: &Portfolio,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create order with correct structure
        let order = Order {
            id: Uuid::new_v4().to_string(),
            symbol: proposal.symbol.clone(),
            side: proposal.side,
            price: proposal.price,
            quantity: proposal.quantity,
            order_type: proposal.order_type,
            timestamp: Utc::now().timestamp_millis(),
        };

        // Track as pending
        self.order_reconciler.track_order(
            order.id.clone(),
            PendingOrder {
                symbol: proposal.symbol.clone(),
                side: proposal.side,
                requested_qty: proposal.quantity,
                filled_qty: Decimal::ZERO,
                filled_but_not_synced: false,
                entry_price: proposal.price,
                filled_at: None,
            },
        );

        // Submit order
        info!(
            "RiskManager: Submitting {:?} order for {} qty {} @ {}",
            proposal.side, proposal.symbol, proposal.quantity, proposal.price
        );

        if let Err(e) = self.order_tx.send(order.clone()).await {
            error!("RiskManager: Failed to send order: {}", e);
            self.order_reconciler
                .remove_order(&order.id, &self.portfolio_state_manager);
            return Err(Box::new(e));
        }

        Ok(())
    }

    pub async fn run(&mut self) {
        info!("RiskManager started with config: {:?}", self.risk_config);

        // Initialize session
        if let Err(e) = self.initialize_session().await {
            error!("RiskManager: Failed to initialize session: {}", e);
        }

        // Ticker for periodic valuation
        let mut valuation_interval = tokio::time::interval(tokio::time::Duration::from_secs(
            self.risk_config.valuation_interval_seconds,
        ));

        // Ticker for periodic portfolio refresh (uses config from PortfolioStateManager)
        // Default: refresh every 2 seconds to keep snapshot fresh
        let refresh_interval_ms = std::env::var("PORTFOLIO_REFRESH_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(2000);
        let mut refresh_interval =
            tokio::time::interval(tokio::time::Duration::from_millis(refresh_interval_ms));

        // Subscribe to Real-Time Order Updates
        let mut order_update_rx = match self.execution_service.subscribe_order_updates().await {
            Ok(rx) => Some(rx),
            Err(e) => {
                error!(
                    "RiskManager: Failed to subscribe to order updates: {}. Pending tracking will be limited.",
                    e
                );
                None
            }
        };

        // Ticker for periodic volatility update
        let mut vol_interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // Every hour

        loop {
            tokio::select! {
                // Periodic volatility refresh
                _ = vol_interval.tick() => {
                    if let Err(e) = self.update_volatility().await {
                        error!("RiskManager: Volatility update failed: {}", e);
                    }
                }

                // Periodic portfolio state refresh
                _ = refresh_interval.tick() => {
                    if let Err(e) = self.handle_command(RiskCommand::RefreshPortfolio).await {
                        error!("RiskManager: Portfolio refresh failed: {}", e);
                    }
                }

                // Listen for Order Updates (handle lag explicitly)
                result = async {
                    if let Some(rx) = &mut order_update_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    match result {
                        Ok(update) => {
                            if let Err(e) = self.handle_command(RiskCommand::OrderUpdate(update)).await {
                                error!("RiskManager: Order update handling failed: {}", e);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(
                                "RiskManager: Order update receiver lagged, missed {} updates! Forcing refresh.",
                                n
                            );
                            if let Err(e) = self.handle_command(RiskCommand::RefreshPortfolio).await {
                                error!("RiskManager: Failed to refresh after lag: {}", e);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            error!("RiskManager: Order update channel closed! Real-time tracking disabled.");
                            order_update_rx = None;
                        }
                    }
                }

                // Periodic valuation
                _ = valuation_interval.tick() => {
                    if let Err(e) = self.handle_command(RiskCommand::ValuationTick).await {
                        error!("RiskManager: Valuation failed: {}", e);
                    }
                }

                // Process trade proposals
                Some(proposal) = self.proposal_rx.recv() => {
                    if let Err(e) = self.handle_command(RiskCommand::ProcessProposal(proposal)).await {
                        error!("RiskManager: Proposal processing failed: {}", e);
                    }
                }

                // External commands (Sentiment, etc.)
                Some(cmd) = self.external_cmd_rx.recv() => {
                    if let Err(e) = self.handle_command(cmd).await {
                        error!("RiskManager: External command processing failed: {}", e);
                    }
                }
            }
        }
    }
}
