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

mod alerts;
mod handler;
mod session;
mod valuation;

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
