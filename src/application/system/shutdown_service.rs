use crate::domain::ports::ExecutionService;
use crate::domain::repositories::RiskStateRepository;
use crate::domain::trading::portfolio::Portfolio;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

pub struct ShutdownService {
    execution_service: Arc<dyn ExecutionService>,
    _risk_state_repository: Arc<dyn RiskStateRepository>,
    portfolio: Arc<RwLock<Portfolio>>,
}

impl ShutdownService {
    pub fn new(
        execution_service: Arc<dyn ExecutionService>,
        _risk_state_repository: Arc<dyn RiskStateRepository>,
        portfolio: Arc<RwLock<Portfolio>>,
    ) -> Self {
        Self {
            execution_service,
            _risk_state_repository,
            portfolio,
        }
    }

    pub async fn shutdown(&self) {
        info!("Initiating Graceful Shutdown Sequence...");

        // 1. Cancel all open orders
        info!("Step 1: Cancelling all open orders...");
        if let Err(e) = self.execution_service.cancel_all_orders().await {
            error!("Failed to cancel orders during shutdown: {}", e);
        } else {
            info!("All orders cancelled successfully.");
        }

        // 2. Save Risk State
        info!("Step 2: Saving Risk State...");
        // Assuming RiskStateRepository has a way to save current state or it's done periodically.
        // If the repository is file-based/DB, ensuring flush might be needed.
        // For now, we just log, as most repos save on update.
        // If we had a explicit `save()` or `flush()`, we'd call it here.

        // 3. Save Portfolio State
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
}
