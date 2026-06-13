use crate::application::risk_management::circuit_breaker_service::{
    CircuitBreakerConfig as ServiceCircuitBreakerConfig, CircuitBreakerService, HaltLevel,
};
use crate::application::risk_management::liquidation_service::LiquidationService;
use crate::application::risk_management::order_reconciler::OrderReconciler;
use crate::application::risk_management::pipeline::validation_pipeline::RiskValidationPipeline;
use crate::application::risk_management::portfolio_valuation_service::PortfolioValuationService;
use crate::application::risk_management::session_manager::SessionManager;

use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::risk_management::state::risk_state_manager::RiskStateManager;
use crate::domain::ports::{ExecutionService, MarketDataService, NotificationService, OrderUpdate};
use crate::domain::repositories::{CandleRepository, RiskStateRepository};
use crate::domain::risk::filters::{
    RiskValidator,
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
use crate::domain::trading::types::{Order, TradeProposal};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock; // Added
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, instrument, warn};

use crate::application::monitoring::connection_health_service::ConnectionHealthService;
use crate::application::monitoring::correlation_service::CorrelationService;
use crate::application::monitoring::performance_monitoring_service::PerformanceMonitoringService;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::commands::RiskCommand;
use crate::config::AssetClass;
use crate::infrastructure::observability::Metrics;

pub use crate::domain::risk::risk_config::{RiskConfig, RiskConfigError};

mod handler;

pub struct RiskManagerDependencies {
    pub execution_service: Arc<dyn ExecutionService>,
    pub market_service: Arc<dyn MarketDataService>,
    pub portfolio_state_manager: Arc<PortfolioStateManager>,
    pub performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    pub correlation_service: Option<Arc<CorrelationService>>,
    pub risk_state_repository: Option<Arc<dyn RiskStateRepository>>,
    pub candle_repository: Option<Arc<dyn CandleRepository>>,
    pub spread_cache: Arc<SpreadCache>,
    pub connection_health_service: Arc<ConnectionHealthService>,
    pub metrics: Metrics,
    pub agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
}

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

    state_manager: RiskStateManager,

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
    symbol_sentiments: HashMap<String, Sentiment>,
    // risk_state_repository removed (moved to state_manager)
    candle_repository: Option<Arc<dyn CandleRepository>>,

    // Services
    metrics: Metrics,
    agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    startup_time: i64,
    alert_webhook_url: Option<String>,
}

impl RiskManager {
    pub fn new(
        proposal_rx: Receiver<TradeProposal>,
        external_cmd_rx: Receiver<RiskCommand>,
        order_tx: Sender<Order>,
        non_pdt_mode: bool,
        asset_class: AssetClass,
        risk_config: RiskConfig,
        deps: RiskManagerDependencies,
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
            deps.risk_state_repository.clone(),
            Decimal::ZERO, // Initialized later in initialize_session
        );

        let volatility_manager = Arc::new(RwLock::new(VolatilityManager::new(
            risk_config.volatility_config.clone(),
        )));

        // Initialize extracted services
        let session_manager = SessionManager::new(
            deps.risk_state_repository.clone(),
            deps.market_service.clone(),
        );

        let portfolio_valuation_service = PortfolioValuationService::new(
            deps.market_service.clone(),
            deps.portfolio_state_manager.clone(),
            volatility_manager.clone(),
            asset_class,
        );

        let liquidation_service = LiquidationService::new(
            Some(order_tx.clone()),
            deps.portfolio_state_manager.clone(),
            deps.market_service.clone(),
            deps.spread_cache.clone(),
        );

        Ok(Self {
            proposal_rx,
            external_cmd_rx,
            order_tx,
            execution_service: deps.execution_service,
            market_service: deps.market_service,
            portfolio_state_manager: deps.portfolio_state_manager,

            asset_class,

            volatility_manager,

            // New Components
            validation_pipeline,
            state_manager,
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
            performance_monitor: deps.performance_monitor,
            correlation_service: deps.correlation_service,

            // halted removed
            daily_pnl: Decimal::ZERO,

            // pending_reservations removed
            connection_health_service: deps.connection_health_service,
            last_quote_timestamp: Utc::now().timestamp_millis(),
            symbol_sentiments: HashMap::new(),
            candle_repository: deps.candle_repository,
            metrics: deps.metrics,
            agent_registry: deps.agent_registry,
            startup_time: Utc::now().timestamp(),
            alert_webhook_url: None,
        })
    }

    /// Persist current risk state to database
    async fn persist_state(&self) {
        self.state_manager.persist().await;
    }

    /// Set webhook URL for alerts
    pub fn set_alert_webhook_url(&mut self, url: Option<String>) {
        self.alert_webhook_url = url;
    }

    /// Set notification service for emergency alerts
    pub fn set_notification_service(&mut self, service: Arc<dyn NotificationService>) {
        self.liquidation_service.set_notification_service(service);
    }

    /// Helper to emit structured tracing event and send asynchronous webhook alert
    fn trigger_alert(&self, level: HaltLevel, reason: &str) {
        // Structured tracing log with dedicated event name
        tracing::error!(
            event = "circuit_breaker_triggered",
            level = ?level,
            reason = %reason,
            "CIRCUIT BREAKER TRIGGERED ({:?}): {}",
            level,
            reason
        );

        if let Some(ref webhook_url) = self.alert_webhook_url {
            let webhook_url = webhook_url.clone();
            let reason = reason.to_string();
            let level_str = format!("{:?}", level);
            let timestamp = Utc::now().to_rfc3339();

            tokio::spawn(async move {
                let payload = serde_json::json!({
                    "event": "circuit_breaker_triggered",
                    "level": level_str,
                    "reason": reason,
                    "timestamp": timestamp,
                    "message": format!("⚠️ [CIRCUIT BREAKER] Level: {}, Reason: {}", level_str, reason)
                });

                let client = reqwest::Client::new();
                match client.post(&webhook_url).json(&payload).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::warn!(
                                "Failed to send alert to webhook. Status: {}",
                                resp.status()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send alert to webhook: {}", e);
                    }
                }
            });
        }
    }

    /// Initialize session tracking with starting equity
    /// Delegates to SessionManager for session lifecycle management
    pub async fn initialize_session(&mut self) -> anyhow::Result<()> {
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

        // Push daily_start_equity to portfolio
        self.portfolio_state_manager
            .update_starting_cash(risk_state.daily_start_equity)
            .await;

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
    pub async fn update_portfolio_valuation(&mut self) -> anyhow::Result<()> {
        // Delegate valuation to PortfolioValuationService
        let (portfolio, current_equity) = self
            .portfolio_valuation_service
            .update_portfolio_valuation(&mut self.current_prices)
            .await?;

        // Update volatility
        let _ = self.portfolio_valuation_service.update_volatility().await;

        // Update High Water Mark via State Manager
        self.state_manager.update(current_equity, Utc::now()).await;

        // Check Risks (Async check)
        // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
        if !self.circuit_breaker_service.is_halted() {
            // SAFEGUARD: Ensure we have prices for ALL positions before running circuit breaker.
            // If we fall back to average_price in total_equity, we might trigger a false drawdown
            // if the asset has appreciated significantly since entry.
            // (e.g. HWM based on $150, but we use AvgPrice $100 -> -33% Drawdown -> PANIC)
            // Note: portfolio is not returned by update_portfolio_valuation but we can get it from state manager
            // actually update_portfolio_valuation returns (Portfolio, Decimal) based on my reading of lines 336-339
            // Wait, looking at lines 336-339 in the original file view:
            // let (_portfolio, current_equity) = self...
            // So I need to capture the portfolio variable.

            let has_missing_prices = portfolio
                .positions
                .keys()
                .any(|symbol| !self.current_prices.contains_key(symbol));

            if has_missing_prices {
                // Log warning but DO NOT trigger circuit breaker
                let missing: Vec<_> = portfolio
                    .positions
                    .keys()
                    .filter(|s| !self.current_prices.contains_key(*s))
                    .collect();
                tracing::warn!(
                    "RiskManager: Skipping Circuit Breaker check due to missing prices for: {:?}",
                    missing
                );
            } else if let Some((level, reason)) = self.check_circuit_breaker(current_equity) {
                let current_level = self.circuit_breaker_service.halt_level();
                if level > current_level {
                    self.trigger_alert(level, &reason);
                    self.circuit_breaker_service.set_halted(level);
                    self.metrics.circuit_breaker_status.set(1.0);

                    // Grace Period: skip emergency liquidation during first 60 seconds
                    if Utc::now().timestamp() - self.startup_time < 60 {
                        warn!(
                            "RiskManager: CIRCUIT BREAKER TRIGGERED ({:?}) during startup grace period. skipping liquidation for stabilization.",
                            level
                        );
                    } else {
                        self.liquidate_portfolio(&reason).await;
                    }
                }
            } else {
                self.metrics.circuit_breaker_status.set(0.0);
            }
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
    pub async fn update_volatility(&self) -> anyhow::Result<()> {
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

                    // Calculate proper True Range (TR)
                    // TR = Max(H-L, |H-Cp|, |L-Cp|)
                    let last = &candles[candles.len() - 1];
                    let prev_close = candles[candles.len() - 2]
                        .close
                        .to_f64()
                        .unwrap_or_else(|| last.open.to_f64().unwrap_or(0.0));

                    let high = last.high.to_f64().unwrap_or(0.0);
                    let low = last.low.to_f64().unwrap_or(0.0);

                    let tr = (high - low)
                        .max((high - prev_close).abs())
                        .max((low - prev_close).abs());

                    if tr > 0.0 {
                        let mut vm = volatility_manager.write().await;
                        let tr_dec = Decimal::from_f64_retain(tr).unwrap_or(Decimal::ZERO);
                        vm.update(tr_dec);
                        debug!(
                            "RiskManager: Volatility updated for {}. Latest TR: {}, Avg: {}",
                            benchmark_string,
                            tr_dec,
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
    pub async fn check_daily_reset(&mut self, current_equity: Decimal) -> bool {
        let old_reset = self.state_manager.get_state().daily_drawdown_reset;

        // Delegate to RiskStateManager
        self.state_manager.check_daily_reset(current_equity);

        let new_reset = self.state_manager.get_state().daily_drawdown_reset;

        if new_reset && !old_reset {
            self.daily_pnl = Decimal::ZERO;
            self.circuit_breaker_service.set_halted(HaltLevel::Normal);
            self.metrics.circuit_breaker_status.set(0.0);

            // Push the new daily_start_equity to portfolio
            let start_equity = self.state_manager.get_state().daily_start_equity;
            self.portfolio_state_manager
                .update_starting_cash(start_equity)
                .await;

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

    /// Skip the startup grace period (for tests that need immediate circuit breaker behavior)
    pub fn skip_startup_grace_period(&mut self) {
        self.startup_time = 0; // Set to epoch so grace period check always passes
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

        let mut agent_health_check_interval =
            tokio::time::interval(std::time::Duration::from_secs(5));

        // Initial Heartbeat
        self.agent_registry
            .update_heartbeat(
                "RiskManager",
                crate::application::monitoring::agent_status::HealthStatus::Healthy,
            )
            .await;

        loop {
            tokio::select! {
                _ = agent_health_check_interval.tick() => {
                    self.agent_registry
                        .update_heartbeat(
                            "RiskManager",
                            if self.is_halted() {
                                crate::application::monitoring::agent_status::HealthStatus::Degraded
                            } else {
                                crate::application::monitoring::agent_status::HealthStatus::Healthy
                            },
                        )
                        .await;

                    self.agent_registry
                         .update_metric(
                             "RiskManager",
                             "circuit_breaker",
                             if self.is_halted() { "HALTED" } else { "NORMAL" }.to_string()
                         )
                         .await;

                    // Add granular risk metrics
                    let state = self.state_manager.get_state();
                    let current_equity = self.portfolio_state_manager.get_snapshot().await.portfolio.total_equity(&self.current_prices);

                    // Drawdown
                    let drawdown = if state.equity_high_water_mark > rust_decimal::Decimal::ZERO {
                        (state.equity_high_water_mark - current_equity) / state.equity_high_water_mark
                    } else {
                        rust_decimal::Decimal::ZERO
                    };
                    self.agent_registry
                        .update_metric("RiskManager", "drawdown", format!("{:.2}%", drawdown * rust_decimal_macros::dec!(100)))
                        .await;

                    // Daily Loss
                    let daily_loss = if state.session_start_equity > rust_decimal::Decimal::ZERO {
                        (state.session_start_equity - current_equity) / state.session_start_equity
                    } else {
                        rust_decimal::Decimal::ZERO
                    };
                    self.agent_registry
                        .update_metric("RiskManager", "daily_loss", format!("{:.2}%", daily_loss * rust_decimal_macros::dec!(100)))
                        .await;

                    // Metrics
                    let snapshot = self.portfolio_state_manager.get_snapshot().await;
                    let current_prices = self.current_prices.clone();
                    let current_equity = snapshot.portfolio.total_equity(&current_prices);
                    let drawdown = if state.equity_high_water_mark > Decimal::ZERO {
                        (state.equity_high_water_mark - current_equity) / state.equity_high_water_mark * dec!(100.0)
                    } else {
                        Decimal::ZERO
                    };

                    let daily_loss = if state.session_start_equity > Decimal::ZERO {
                        (state.session_start_equity - current_equity) / state.session_start_equity * dec!(100.0)
                    } else {
                        Decimal::ZERO
                    };

                    self.agent_registry
                        .update_metric("RiskManager", "drawdown", format!("{:.2}%", drawdown))
                        .await;

                    self.agent_registry
                        .update_metric("RiskManager", "daily_loss", format!("{:.2}%", daily_loss))
                        .await;

                    // Halt Level
                    let level = self.circuit_breaker_service.halt_level();
                    self.agent_registry
                        .update_metric("RiskManager", "halt_level", format!("{:?}", level))
                        .await;

                    // Consecutive Losses
                    self.agent_registry
                        .update_metric("RiskManager", "consecutive_losses", state.consecutive_losses.to_string())
                        .await;

                    // Circuit Breaker Status (Binary for easier detection)
                    let is_halted = level == HaltLevel::Reduced || level == HaltLevel::FullHalt;
                    self.agent_registry
                        .update_metric("RiskManager", "circuit_breaker", if is_halted { "HALTED".to_string() } else { "NORMAL".to_string() })
                        .await;
                }

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
