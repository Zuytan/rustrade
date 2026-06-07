use anyhow::Result;
use async_trait::async_trait;
use rustrade::application::system::shutdown_service::ShutdownService;
use rustrade::domain::repositories::RiskStateRepository;
use rustrade::domain::trading::portfolio::Portfolio;
use std::sync::Arc;
use tokio::sync::RwLock;

mod test_utils;

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
    let mock_execution = Arc::new(test_utils::MockExecutionService::new());
    let mock_risk = Arc::new(MockRiskRepo);
    let portfolio = Arc::new(RwLock::new(Portfolio::new()));
    let mock_market = Arc::new(MockMarketService);
    let spread_cache =
        Arc::new(rustrade::application::market_data::spread_cache::SpreadCache::new());
    let config =
        rustrade::application::system::shutdown_service::EmergencyShutdownConfig::default();

    let service = ShutdownService::new(
        mock_execution.clone(),
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
        mock_execution
            .cancel_all_called
            .load(std::sync::atomic::Ordering::SeqCst),
        "cancel_all_orders should be called on shutdown"
    );
}
