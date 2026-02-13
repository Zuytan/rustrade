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
    // Updated signature
    async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<()> {
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

// Minimal Mock Market Service
struct MockMarketService;
#[async_trait]
impl rustrade::domain::ports::MarketDataService for MockMarketService {
    async fn subscribe(
        &self,
        _s: Vec<String>,
    ) -> Result<tokio::sync::mpsc::Receiver<rustrade::domain::trading::types::MarketEvent>> {
        let (_, rx) = tokio::sync::mpsc::channel(1);
        Ok(rx)
    }
    async fn get_tradable_assets(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn get_top_movers(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn get_prices(
        &self,
        _s: Vec<String>,
    ) -> Result<std::collections::HashMap<String, rust_decimal::Decimal>> {
        Ok(std::collections::HashMap::new())
    }
    async fn get_historical_bars(
        &self,
        _s: &str,
        _st: chrono::DateTime<chrono::Utc>,
        _e: chrono::DateTime<chrono::Utc>,
        _t: &str,
    ) -> Result<Vec<rustrade::domain::trading::types::Candle>> {
        Ok(vec![])
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
    let mock_market = Arc::new(MockMarketService);
    let spread_cache =
        Arc::new(rustrade::application::market_data::spread_cache::SpreadCache::new());
    let config =
        rustrade::application::system::shutdown_service::EmergencyShutdownConfig::default();

    let service = ShutdownService::new(
        mock_execution,
        mock_risk,
        portfolio,
        mock_market,
        spread_cache,
        config,
    );

    // Trigger shutdown
    service.shutdown().await;

    // Verify
    assert!(
        *cancel_called.lock().unwrap(),
        "cancel_all_orders should be called on shutdown"
    );
}
