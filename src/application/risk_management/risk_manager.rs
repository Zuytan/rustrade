use crate::application::risk_management::circuit_breaker_service::{
    CircuitBreakerConfig as ServiceCircuitBreakerConfig, CircuitBreakerService, HaltLevel,
};
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
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use crate::application::monitoring::connection_health_service::ConnectionHealthService;
use crate::application::monitoring::correlation_service::CorrelationService;
use crate::application::monitoring::performance_monitoring_service::PerformanceMonitoringService;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::commands::RiskCommand;
use crate::config::AssetClass;
use crate::infrastructure::observability::Metrics;

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

    // pending_orders removed - replaced by order_reconciler

    // Runtime flags
    // halted moved to CircuitBreakerService
    daily_pnl: Decimal,

    // NEW Resilience State
    connection_health_service: Arc<ConnectionHealthService>,
    last_quote_timestamp: i64,

    // Cache
    current_prices: HashMap<String, Decimal>,
    // pending_reservations moved to OrderReconciler
    current_sentiment: Option<Sentiment>,
    // risk_state_repository removed (moved to state_manager)
    candle_repository: Option<Arc<dyn CandleRepository>>,

    // Services
    #[allow(dead_code)] // Reserved for LiquidationService panic-mode exits (blind liquidation)
    spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    metrics: Metrics,
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
        connection_health_service: Arc<ConnectionHealthService>,
        metrics: Metrics,
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

        let liquidation_service = LiquidationService::new(
            order_tx.clone(),
            portfolio_state_manager.clone(),
            market_service.clone(),
            spread_cache.clone(),
        );

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

            // pending_orders removed
            current_prices: HashMap::new(),
            performance_monitor,
            correlation_service,

            // halted removed
            daily_pnl: Decimal::ZERO,

            // pending_reservations removed
            current_sentiment: None,
            // risk_state_repository removed
            candle_repository,
            spread_cache,
            connection_health_service,
            last_quote_timestamp: Utc::now().timestamp(),
            metrics,
        })
    }

    /// Persist current risk state to database
    async fn persist_state(&self) {
        self.state_manager.persist().await;
    }

    /// Initialize session tracking with starting equity
    /// Delegates to SessionManager for session lifecycle management
    pub async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Wait for Portfolio Synchronization (prevents false drawdown trigger)
        info!("RiskManager: Waiting for portfolio synchronization...");
        let mut attempts = 0;
        loop {
            let snapshot = self.portfolio_state_manager.refresh().await?;
            if snapshot.portfolio.synchronized {
                info!(
                    "RiskManager: Portfolio synchronized. Proceeding with session initialization."
                );
                break;
            }

            attempts += 1;
            if attempts % 20 == 0 {
                warn!(
                    "RiskManager: Still waiting for portfolio synchronization ({}/20s)...",
                    attempts
                );
            }

            if attempts > 60 {
                // ~60 seconds timeout
                crate::infrastructure::core::circuit_breaker::CircuitBreaker::new(
                    "PortfolioSync",
                    1,
                    1,
                    std::time::Duration::from_secs(1),
                );
                warn!(
                    "RiskManager: Portfolio synchronization timed out. Proceeding with potentially stale data."
                );
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        }

        // Get portfolio snapshot
        let snapshot = self.portfolio_state_manager.refresh().await?;

        // Delegate session initialization to SessionManager
        let risk_state = self
            .session_manager
            .initialize_session(&snapshot.portfolio, &mut self.current_prices)
            .await?;

        // Sync state manager
        *self.state_manager.get_state_mut() = risk_state.clone();

        info!(
            "RiskManager: Session initialized. Equity: {}, Daily Start: {}, HWM: {}",
            self.state_manager.get_state().session_start_equity,
            self.state_manager.get_state().daily_start_equity,
            self.state_manager.get_state().equity_high_water_mark
        );

        Ok(())
    }

    /// Check if circuit breaker should trigger; returns level and message when triggered.
    fn check_circuit_breaker(&self, current_equity: Decimal) -> Option<(HaltLevel, String)> {
        self.circuit_breaker_service
            .check_circuit_breaker(self.state_manager.get_state(), current_equity)
    }

    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    async fn handle_order_update(&mut self, update: OrderUpdate) -> bool {
        let (state_changed, token) = self
            .order_reconciler
            .handle_order_update(&update, self.state_manager.get_state_mut());

        // Release reservation token synchronously in this async context
        if let Some(t) = token {
            self.portfolio_state_manager.release_reservation(t).await;
        }

        state_changed
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

        // Check Risks (Async check)
        // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
        // Check Risks (Async check)
        // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
        if !self.circuit_breaker_service.is_halted()
            && let Some((level, reason)) = self.check_circuit_breaker(current_equity)
        {
            tracing::error!(
                "RiskManager MONITOR: CIRCUIT BREAKER TRIGGERED ({:?}): {}",
                level,
                reason
            );
            self.circuit_breaker_service.set_halted(level);
            self.metrics.circuit_breaker_status.set(1.0);
            self.liquidate_portfolio(&reason).await;
        } else if !self.circuit_breaker_service.is_halted() {
            self.metrics.circuit_breaker_status.set(0.0);
        }

        // Capture performance snapshot if monitor available
        if let Some(monitor) = &self.performance_monitor {
            for sym in self.current_prices.keys() {
                let _ = monitor.capture_snapshot(sym).await;
            }
        }

        Ok(())
    }

    /// Update volatility manager with latest ATR/Benchmark data (Non-blocking)
    pub async fn update_volatility(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Choose benchmark symbol based on asset class
        let benchmark = match self.asset_class {
            AssetClass::Crypto => "BTC/USDT",
            _ => "SPY",
        };

        let market_service = self.market_service.clone();
        let volatility_manager = self.volatility_manager.clone();
        let benchmark_string = benchmark.to_string();

        // Spawn background task to avoid blocking the event loop with network I/O
        tokio::spawn(async move {
            let now = Utc::now();
            let start = now - chrono::Duration::days(30); // 30 days to get enough candles

            match market_service
                .get_historical_bars(&benchmark_string, start, now, "1D")
                .await
            {
                Ok(candles) => {
                    if candles.len() < 2 {
                        return;
                    }

                    // Calculate a simple True Range for the latest candle
                    // TR = Max(H-L, H-Cp, L-Cp)
                    // For simplicity here, we take H-L
                    let last = &candles[candles.len() - 1];
                    let high = last.high.to_f64().unwrap_or(0.0);
                    let low = last.low.to_f64().unwrap_or(0.0);
                    let range = high - low;

                    if range > 0.0 {
                        let mut vm = volatility_manager.write().await;
                        let range_dec = Decimal::from_f64_retain(range).unwrap_or(Decimal::ZERO);
                        vm.update(range_dec);
                        debug!(
                            "RiskManager: Volatility updated for {}. Latest range: {}, Avg: {}",
                            benchmark_string,
                            range_dec,
                            vm.get_average_volatility()
                        );
                    }
                }
                Err(e) => {
                    warn!("RiskManager: Failed to fetch volatility data: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Emergency liquidation of entire portfolio
    /// Delegates to LiquidationService for emergency liquidation logic
    #[instrument(skip(self))]
    async fn liquidate_portfolio(&mut self, reason: &str) {
        // Delegate liquidation to LiquidationService
        self.liquidation_service
            .liquidate_portfolio(reason, &self.current_prices)
            .await;
    }

    /// Check if we need to reset session stats (for 24/7 Crypto markets)
    pub fn check_daily_reset(&mut self, current_equity: Decimal) -> bool {
        let old_reset = self.state_manager.get_state().daily_drawdown_reset;

        // Delegate to RiskStateManager
        self.state_manager.check_daily_reset(current_equity);

        let new_reset = self.state_manager.get_state().daily_drawdown_reset;

        if new_reset && !old_reset {
            self.daily_pnl = Decimal::ZERO;
            self.circuit_breaker_service.set_halted(HaltLevel::Normal);
            self.metrics.circuit_breaker_status.set(0.0);
            return true;
        }

        // Check if reference date changed (handled by state manager logic above, so implied by new_reset usually)
        // But if we want to be safe about updated_at check:
        if self.asset_class == AssetClass::Crypto
            && self.state_manager.get_state().updated_at >= Utc::now().timestamp() - 1
        {
            return true;
        }
        false
    }

    /// Cleanup tentative filled orders and release reservations
    async fn reconcile_pending_orders(&mut self, portfolio: &Portfolio) {
        let tokens = self.order_reconciler.reconcile_pending_orders(portfolio);

        // Release all reservation tokens in batch
        self.portfolio_state_manager
            .release_reservations(tokens)
            .await;
    }

    pub fn is_halted(&self) -> bool {
        self.circuit_breaker_service.is_halted()
    }

    pub fn get_state(&self) -> &RiskState {
        self.state_manager.get_state()
    }

    pub fn get_state_mut(&mut self) -> &mut RiskState {
        self.state_manager.get_state_mut()
    }

    // ============================================================================
    // COMMAND PATTERN HANDLERS
    // ============================================================================

    /// Handle internal commands
    pub async fn handle_command(
        &mut self,
        command: RiskCommand,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match command {
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
                self.circuit_breaker_service.set_halted(HaltLevel::FullHalt);
                self.metrics.circuit_breaker_status.set(1.0);
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
    #[instrument(skip(self, update), fields(symbol = %update.symbol, order_id = %update.order_id, status = ?update.status))]
    async fn cmd_handle_order_update(
        &mut self,
        update: OrderUpdate,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.handle_order_update(update).await {
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
        }

        // Always reconcile pending orders regardless of circuit breaker state.
        // Stale reservations must be released to avoid permanently locking capital.
        let snapshot = self.portfolio_state_manager.get_snapshot().await;
        self.reconcile_pending_orders(&snapshot.portfolio).await;

        Ok(())
    }

    /// Handle trade proposal command
    #[instrument(skip(self, proposal), fields(symbol = %proposal.symbol, side = ?proposal.side))]
    async fn cmd_handle_proposal(
        &mut self,
        proposal: TradeProposal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let level = self.circuit_breaker_service.halt_level();
        if level == HaltLevel::Reduced || level == HaltLevel::FullHalt {
            info!(
                "RiskManager: Trading HALTED ({:?}). Rejecting proposal for {}",
                level, proposal.symbol
            );
            return Ok(());
        }
        let mut proposal = proposal;
        if level == HaltLevel::Warning {
            let mult = rust_decimal::Decimal::from_f64_retain(HaltLevel::Warning.size_multiplier())
                .unwrap_or(Decimal::ONE);
            proposal.quantity = (proposal.quantity * mult).round_dp(4);
            if proposal.quantity <= Decimal::ZERO {
                info!(
                    "RiskManager: Proposal for {} scaled to zero under Warning level, skipping",
                    proposal.symbol
                );
                return Ok(());
            }
            debug!(
                "RiskManager: Circuit breaker Warning: reduced proposal size for {} by 50%",
                proposal.symbol
            );
        }

        // --- STALE DATA GUARD ---
        // Use ConnectionHealthService as the single source of truth for market data freshness.
        // It properly tracks the last received data event independently of proposal processing.
        if self
            .connection_health_service
            .get_market_data_status()
            .await
            == crate::application::monitoring::connection_health_service::ConnectionStatus::Offline
        {
            info!(
                "RiskManager: Market Data OFFLINE. Rejecting proposal for {}",
                proposal.symbol
            );
            return Ok(());
        }
        // -------------------------

        info!("RiskManager: reviewing proposal {:?}", proposal);

        // Update current price
        let now = Utc::now().timestamp();
        self.current_prices
            .insert(proposal.symbol.clone(), proposal.price);
        self.last_quote_timestamp = now; // Track for metrics/debugging

        // Get portfolio snapshot
        let mut snapshot = self.portfolio_state_manager.get_snapshot().await;

        // Refresh if stale
        if self.portfolio_state_manager.is_stale(&snapshot) {
            snapshot = self.portfolio_state_manager.refresh().await?;
        }

        // Reconcile pending orders
        self.reconcile_pending_orders(&snapshot.portfolio).await;

        // Calculate current equity
        let current_equity = snapshot.portfolio.total_equity(&self.current_prices);

        // Update high water mark
        if current_equity > self.state_manager.get_state().equity_high_water_mark {
            self.state_manager.get_state_mut().equity_high_water_mark = current_equity;
        }

        // Check daily reset
        if self.check_daily_reset(current_equity) {
            self.persist_state().await;
        }

        // Circuit breaker check (Trigger Liquidation logic)
        if let Some((level, reason)) = self.check_circuit_breaker(current_equity) {
            error!(
                "RiskManager: CIRCUIT BREAKER TRIGGERED ({:?}) - {}",
                level, reason
            );
            self.circuit_breaker_service.set_halted(level);
            self.metrics.circuit_breaker_status.set(1.0);
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
            service.get_correlation_matrix(&symbols).await.ok()
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

        let available_cash = snapshot.available_cash();

        let ctx = ValidationContext::new(
            &proposal,
            &snapshot.portfolio,
            current_equity,
            &self.current_prices,
            self.state_manager.get_state(),
            self.current_sentiment.as_ref(),
            correlation_matrix.as_ref(), // Pass pre-calculated matrix
            volatility_multiplier,
            pending_exposure,
            available_cash,
            candles_ref, // Pass recent candles from CandleRepository for PriceAnomalyValidator
        );

        // Execute Pipeline
        match self.validation_pipeline.validate(&ctx).await {
            ValidationResult::Approve => {
                // Reserve exposure for BUY orders to prevent over-allocation.
                // This ensures that subsequent proposals see reduced available_cash
                // and won't exceed the actual balance at the broker.
                let reservation_token = if proposal.side == OrderSide::Buy {
                    let order_cost = proposal.price * proposal.quantity;
                    match self
                        .portfolio_state_manager
                        .reserve_exposure(&proposal.symbol, order_cost, snapshot.version)
                        .await
                    {
                        Ok(token) => {
                            info!(
                                "RiskManager: Reserved ${} for {} (token: {})",
                                order_cost,
                                proposal.symbol,
                                &token.id[..8]
                            );
                            Some(token)
                        }
                        Err(e) => {
                            // Version conflict or insufficient funds after reservation accounting.
                            // Retry once with a fresh snapshot.
                            match self.portfolio_state_manager.refresh().await {
                                Ok(fresh) => {
                                    match self
                                        .portfolio_state_manager
                                        .reserve_exposure(
                                            &proposal.symbol,
                                            order_cost,
                                            fresh.version,
                                        )
                                        .await
                                    {
                                        Ok(token) => Some(token),
                                        Err(retry_err) => {
                                            info!(
                                                "RiskManager: Reservation failed for {} after retry: {}. \
                                                 Rejecting to prevent over-allocation.",
                                                proposal.symbol, retry_err
                                            );
                                            return Ok(());
                                        }
                                    }
                                }
                                Err(refresh_err) => {
                                    info!(
                                        "RiskManager: Portfolio refresh failed during reservation for {}: {}. \
                                         Original error: {}. Rejecting proposal.",
                                        proposal.symbol, refresh_err, e
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    }
                } else {
                    None
                };

                // All checks passed — submit order with reservation
                self.execute_proposal_internal(proposal, reservation_token)
                    .await?;
            }
            ValidationResult::Reject(reason) => {
                info!(
                    "RiskManager: Rejecting {:?} order for {} - {}",
                    proposal.side, proposal.symbol, reason
                );
            }
        }

        Ok(())
    }

    /// Internal proposal execution logic (extracted from run())
    ///
    /// Accepts an optional `ReservationToken` for BUY orders that tracks the
    /// reserved capital in `PortfolioStateManager`. The reservation is released
    /// automatically when the order completes (fill/reject/cancel) via
    /// `OrderReconciler::remove_order`.
    async fn execute_proposal_internal(
        &mut self,
        proposal: TradeProposal,
        reservation_token: Option<
            crate::application::monitoring::portfolio_state_manager::ReservationToken,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create order with correct structure
        let order = Order {
            id: Uuid::new_v4().to_string(),
            symbol: proposal.symbol.clone(),
            side: proposal.side,
            price: proposal.price,
            quantity: proposal.quantity,
            order_type: proposal.order_type,
            status: crate::domain::trading::types::OrderStatus::Pending,
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
                submitted_at: Utc::now().timestamp_millis(),
            },
        );

        // Associate reservation token with order so it is released on completion
        if let Some(token) = reservation_token {
            self.order_reconciler
                .add_reservation(order.id.clone(), token);
        }

        // Submit order
        info!(
            symbol = %proposal.symbol,
            side = ?proposal.side,
            qty = %proposal.quantity,
            price = %proposal.price,
            "RiskManager: Submitting order"
        );

        if let Err(e) = self.order_tx.send(order.clone()).await {
            error!(error = %e, "RiskManager: Failed to send order");
            if let Some(token) = self.order_reconciler.remove_order(&order.id) {
                self.portfolio_state_manager
                    .release_reservation(token)
                    .await;
            }
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

        // Subscribe to Health Events ONCE before the loop to avoid missing events.
        // Creating a new subscriber inside select! causes a race condition where
        // events broadcast between iterations are permanently lost.
        let mut health_rx = self.connection_health_service.subscribe();

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

                // Listen for Health Events (using persistent subscriber)
                Ok(health_event) = health_rx.recv() => {
                    if health_event.component == "MarketData" && health_event.status == crate::application::monitoring::connection_health_service::ConnectionStatus::Offline {
                        warn!("RiskManager: Detected Market Data OFFLINE via HealthService. Safeguarding...");
                        // Future: Could force reconcile or tighter stops here
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
