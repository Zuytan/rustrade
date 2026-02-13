use anyhow::Result;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{error, info, warn};

pub mod shutdown_service;

use crate::application::bootstrap::{
    agents::AgentsBootstrap,
    persistence::{PersistenceBootstrap, PersistenceHandle},
    services::{ServicesBootstrap, ServicesHandle},
};

use crate::application::{
    agents::{analyst::AnalystCommand, sentinel::SentinelCommand},
    market_data::spread_cache::SpreadCache,
    monitoring::connection_health_service::ConnectionHealthService,
    monitoring::performance_monitoring_service::PerformanceMonitoringService,
    optimization::adaptive_optimization_service::AdaptiveOptimizationService,
    risk_management::commands::RiskCommand,
    system::shutdown_service::ShutdownService, // Import ShutdownService
};
use crate::config::Config;
use crate::infrastructure::observability::Metrics;

use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::{
    CandleRepository, RiskStateRepository, StrategyRepository, TradeRepository,
};
use crate::domain::sentiment::Sentiment;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Candle, TradeProposal};

pub struct SystemHandle {
    pub sentinel_cmd_tx: mpsc::Sender<SentinelCommand>,
    pub risk_cmd_tx: mpsc::Sender<RiskCommand>,
    pub analyst_cmd_tx: mpsc::Sender<AnalystCommand>,
    pub proposal_tx: mpsc::Sender<TradeProposal>,
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub candle_rx: broadcast::Receiver<Candle>,
    pub sentiment_rx: broadcast::Receiver<Sentiment>,
    pub news_rx: broadcast::Receiver<crate::domain::listener::NewsEvent>,
    pub connection_health_service: Arc<ConnectionHealthService>,
    pub strategy_mode: crate::domain::market::strategy_config::StrategyMode,
    pub risk_appetite: Option<crate::domain::risk::risk_appetite::RiskAppetite>,
    pub metrics: Metrics,
    pub agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
}

pub struct Application {
    pub config: Config,
    // We keep these handles to prevent drop, and for access if needed
    pub persistence: PersistenceHandle,
    pub services: ServicesHandle,

    // We also expose the flattened fields for backward compatibility with main.rs or other users
    // if they access them directly. If main.rs only uses .build() and .start(), these might not be needed public?
    // Let's check main.rs or assumption.
    // The previous Application struct exposed them.
    // To be safe, we can implement Deref or just keep the references.
    pub market_service: Arc<dyn MarketDataService>,
    pub execution_service: Arc<dyn ExecutionService>,
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub order_repository: Arc<dyn TradeRepository>,
    pub candle_repository: Option<Arc<dyn CandleRepository>>,
    pub strategy_repository: Arc<dyn StrategyRepository>,
    pub adaptive_optimization_service: Option<Arc<AdaptiveOptimizationService>>,
    pub performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    pub spread_cache: Arc<SpreadCache>,
    pub risk_state_repository: Arc<dyn RiskStateRepository>,
    pub connection_health_service: Arc<ConnectionHealthService>,
    pub metrics: Metrics,
    pub agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
}

impl Application {
    pub async fn build(config: Config) -> Result<Self> {
        info!("Building Rustrade Application (Mode: {:?})...", config.mode);

        // 1. Initialize Shared State
        let initial_portfolio = Portfolio::new();
        // initial_portfolio.cash = config.initial_cash; // Removed dangerous default
        let portfolio = Arc::new(RwLock::new(initial_portfolio));

        // 1. Initialize Metrics & Persistence
        let metrics = Metrics::new()?;
        let persistence = PersistenceBootstrap::init().await?;

        let agent_registry = Arc::new(
            crate::application::monitoring::agent_status::AgentStatusRegistry::new(metrics.clone()),
        );

        // 3. Initialize Services (needs Persistence and Portfolio)
        let services =
            ServicesBootstrap::init(&config, &persistence, portfolio.clone(), metrics.clone())
                .await?;

        // Log Risk Appetite configuration
        if let Some(ref appetite) = config.risk_appetite {
            info!(
                "Risk Appetite Score: {} ({:?}) - Calculated Parameters: risk_per_trade={:.2}%, trailing_stop={:.1}x, rsi_threshold={:.0}, max_position={:.1}%",
                appetite.score(),
                appetite.profile(),
                config.risk_per_trade_percent * dec!(100.0),
                config.trailing_stop_atr_multiplier,
                config.rsi_threshold,
                config.max_position_size_pct * dec!(100.0)
            );
        }

        Ok(Self {
            config,
            market_service: services.market_service.clone(),
            execution_service: services.execution_service.clone(),
            portfolio,
            order_repository: persistence.order_repository.clone(),
            candle_repository: Some(persistence.candle_repository.clone()),
            strategy_repository: persistence.strategy_repository.clone(),
            adaptive_optimization_service: services.adaptive_optimization_service.clone(),
            performance_monitor: services.performance_monitor.clone(),
            spread_cache: services.spread_cache.clone(),
            risk_state_repository: persistence.risk_state_repository.clone(),
            connection_health_service: services.connection_health_service.clone(),
            metrics: metrics.clone(),
            persistence,
            services,
            agent_registry,
        })
    }

    pub async fn start(self) -> Result<SystemHandle> {
        info!("Starting Agents...");

        // Initial Portfolio Sync
        info!("Synchronizing Portfolio State...");
        match self.execution_service.get_portfolio().await {
            Ok(initial_portfolio) => {
                let mut pf = self.portfolio.write().await;
                *pf = initial_portfolio;
                info!(
                    "Portfolio synchronized. Cash: ${}, Positions: {}",
                    pf.cash,
                    pf.positions.len()
                );
            }
            Err(e) => {
                warn!(
                    "Failed to fetch initial portfolio state: {}. Using default/empty state.",
                    e
                );
            }
        }

        // Initialize Agents
        let agents = AgentsBootstrap::init(
            &self.config,
            &self.services,
            &self.persistence,
            self.portfolio.clone(),
            self.connection_health_service.clone(),
            self.metrics.clone(),
            self.agent_registry.clone(),
        )
        .await?;

        // Initialize and Start Shutdown Service
        let flatten_on_exit = std::env::var("FLATTEN_ON_EXIT")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let shutdown_config =
            crate::application::system::shutdown_service::EmergencyShutdownConfig {
                flatten_on_exit,
                liquidation_timeout_ms: 10000,
            };

        let shutdown_service = Arc::new(ShutdownService::new(
            self.execution_service.clone(),
            self.risk_state_repository.clone(),
            self.portfolio.clone(),
            self.market_service.clone(),
            self.spread_cache.clone(),
            shutdown_config,
        ));

        let service_clone = shutdown_service.clone();
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    info!("Received Ctrl+C signal.");
                    service_clone.shutdown().await;
                    info!("Shutdown sequence completed. Exiting.");
                    std::process::exit(0);
                }
                Err(err) => {
                    error!("Unable to listen for shutdown signal: {}", err);
                }
            }
        });

        Ok(SystemHandle {
            sentinel_cmd_tx: agents.sentinel_cmd_tx,
            risk_cmd_tx: agents.risk_cmd_tx,
            analyst_cmd_tx: agents.analyst_cmd_tx,
            proposal_tx: agents.proposal_tx,
            portfolio: self.portfolio.clone(),
            candle_rx: agents.candle_rx,
            sentiment_rx: agents.sentiment_rx,
            news_rx: agents.news_rx,
            connection_health_service: self.connection_health_service.clone(),
            strategy_mode: self.config.strategy_mode,
            risk_appetite: self.config.risk_appetite,
            metrics: self.metrics.clone(),
            agent_registry: self.agent_registry.clone(),
        })
    }
}
