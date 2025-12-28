use crate::domain::portfolio::Portfolio;
use crate::domain::types::{MarketEvent, Order};
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc::Receiver;

// Need async_trait for async functions in traits
#[async_trait]
pub trait MarketDataService: Send + Sync {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>>;
    async fn get_top_movers(&self) -> Result<Vec<String>>;
    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, rust_decimal::Decimal>>;
    async fn get_historical_bars(
        &self,
        symbol: &str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        timeframe: &str,
    ) -> Result<Vec<crate::domain::types::Candle>>;
}

#[async_trait]
pub trait ExecutionService: Send + Sync {
    async fn execute(&self, order: Order) -> Result<()>;
    async fn get_portfolio(&self) -> Result<Portfolio>;
    async fn get_today_orders(&self) -> Result<Vec<Order>>;
}
