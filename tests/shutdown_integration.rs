use anyhow::Result;
use async_trait::async_trait;
use rustrade::application::system::shutdown_service::ShutdownService;
use rustrade::domain::ports::ExecutionService;
use rustrade::domain::repositories::RiskStateRepository;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::Order;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

// Mock Execution Service
struct MockExecutionService {
    cancel_all_called: Arc<Mutex<bool>>,
}

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute(&self, _order: Order) -> Result<()> {
        Ok(())
    }
    async fn get_portfolio(&self) -> Result<Portfolio> {
        Ok(Portfolio::new())
    }
    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn cancel_order(&self, _order_id: &str) -> Result<()> {
        Ok(())
    }
    async fn cancel_all_orders(&self) -> Result<()> {
        let mut called = self.cancel_all_called.lock().unwrap();
        *called = true;
        Ok(())
    }
    async fn subscribe_order_updates(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<rustrade::domain::ports::OrderUpdate>> {
        let (_tx, rx) = tokio::sync::broadcast::channel(1);
        Ok(rx)
    }
}

// Mock Risk Repository
struct MockRiskRepo;
#[async_trait]
impl RiskStateRepository for MockRiskRepo {
    async fn save(&self, _state: &rustrade::domain::risk::state::RiskState) -> Result<()> {
        Ok(())
    }
    async fn load(&self, _id: &str) -> Result<Option<rustrade::domain::risk::state::RiskState>> {
        Ok(None)
    }
}

#[tokio::test]
async fn test_graceful_shutdown_cancels_orders() {
    let cancel_called = Arc::new(Mutex::new(false));
    let mock_execution = Arc::new(MockExecutionService {
        cancel_all_called: cancel_called.clone(),
    });
    let mock_risk = Arc::new(MockRiskRepo);
    let portfolio = Arc::new(RwLock::new(Portfolio::new()));

    let service = ShutdownService::new(mock_execution, mock_risk, portfolio);

    // Trigger shutdown
    service.shutdown().await;

    // Verify
    assert!(
        *cancel_called.lock().unwrap(),
        "cancel_all_orders should be called on shutdown"
    );
}
