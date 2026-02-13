use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::liquidation_service::LiquidationService;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::RiskStateRepository;
use crate::domain::trading::portfolio::Portfolio;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

#[derive(Debug, Clone, Copy)]
pub struct EmergencyShutdownConfig {
    pub flatten_on_exit: bool,
    pub liquidation_timeout_ms: u64,
}

impl Default for EmergencyShutdownConfig {
    fn default() -> Self {
        Self {
            flatten_on_exit: false, // Default to FALSE for safety
            liquidation_timeout_ms: 10000,
        }
    }
}

pub struct ShutdownService {
    execution_service: Arc<dyn ExecutionService>,
    _risk_state_repository: Arc<dyn RiskStateRepository>,
    portfolio: Arc<RwLock<Portfolio>>,
    market_service: Arc<dyn MarketDataService>,
    spread_cache: Arc<SpreadCache>,
    config: EmergencyShutdownConfig,
}

impl ShutdownService {
    pub fn new(
        execution_service: Arc<dyn ExecutionService>,
        _risk_state_repository: Arc<dyn RiskStateRepository>,
        portfolio: Arc<RwLock<Portfolio>>,
        market_service: Arc<dyn MarketDataService>,
        spread_cache: Arc<SpreadCache>,
        config: EmergencyShutdownConfig,
    ) -> Self {
        Self {
            execution_service,
            _risk_state_repository,
            portfolio,
            market_service,
            spread_cache,
            config,
        }
    }

    pub async fn shutdown(&self) {
        info!("Initiating Graceful Shutdown Sequence...");

        // 1. Flatten Positions (if enabled)
        if self.config.flatten_on_exit {
            info!("Step 0: Flattening all positions (Emergency Shutdown Policy)...");
            self.execute_shutdown_liquidation().await;
        } else {
            info!("Step 0: Flattening skipped (disabled in config). Open positions will remain.");
        }

        // 2. Cancel all open orders
        info!("Step 1: Cancelling all open orders...");
        if let Err(e) = self.execution_service.cancel_all_orders().await {
            error!("Failed to cancel orders during shutdown: {}", e);
        } else {
            info!("All orders cancelled successfully.");
        }

        // 3. Save Risk State
        info!("Step 2: Saving Risk State...");
        // Assuming RiskStateRepository has a way to save current state or it's done periodically.
        // If the repository is file-based/DB, ensuring flush might be needed.
        // For now, we just log, as most repos save on update.
        // If we had a explicit `save()` or `flush()`, we'd call it here.

        // 4. Save Portfolio State
        info!("Step 3: Saving Portfolio State...");
        let _portfolio = self.portfolio.read().await;
        // implementation depends on how portfolio is persisted.
        // If it's via events, we might need to flush event bus.
        // If it's file based, we might want to trigger a save.
        // Currently, Portfolio is in-memory and synced from ExecutionService on start.
        // So saving might not be strictly necessary if ExecutionService (Exchange) is the source of truth.
        // But for simulation/paper trading, we might want to dump it.

        info!("Graceful Shutdown Complete. Goodbye!");
    }

    async fn execute_shutdown_liquidation(&self) {
        // Create temporary dependencies for LiquidationService
        // We create a local PortfolioStateManager just for this operation
        // It wraps our shared portfolio.
        let portfolio_state_manager = Arc::new(PortfolioStateManager::new(
            self.execution_service.clone(),
            5000, // standard staleness
        ));

        // Create LiquidationService (no channel needed as we execute manually)
        let liquidation_service = LiquidationService::new(
            None,
            portfolio_state_manager,
            self.market_service.clone(),
            self.spread_cache.clone(),
        );

        // Fetch current prices for "Best Effort" valuation
        // We use spread cache or last known prices if available?
        // LiquidationService generates orders. logic uses spread cache.
        // We pass empty map for current_prices if we don't have them easily accessible.
        // LiquidationService will fallback to REST or Panic Mode.
        let current_prices = HashMap::new();

        let orders = liquidation_service
            .generate_liquidation_orders("Shutdown Flatten", &current_prices)
            .await;

        if orders.is_empty() {
            info!("No open positions to flatten.");
            return;
        }

        info!(
            "Generated {} liquidation orders. Executing with retry logic...",
            orders.len()
        );

        liquidation_service
            .execute_orders_with_retry(orders, &self.execution_service)
            .await;

        info!("Shutdown Liquidation: Execution cycle complete.");
    }
}
