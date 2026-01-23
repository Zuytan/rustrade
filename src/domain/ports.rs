use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{MarketEvent, Order, OrderSide, OrderStatus};
use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::{broadcast, mpsc::Receiver};

// Need async_trait for async functions in traits
#[async_trait]
pub trait MarketDataService: Send + Sync {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>>;
    async fn get_top_movers(&self) -> Result<Vec<String>>;
    /// Fetch all tradable assets from the exchange (dynamic discovery)
    async fn get_tradable_assets(&self) -> Result<Vec<String>>;
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
    ) -> Result<Vec<crate::domain::trading::types::Candle>>;
}

#[async_trait]
pub trait ExecutionService: Send + Sync {
    async fn execute(&self, order: Order) -> Result<()>;
    async fn get_portfolio(&self) -> Result<Portfolio>;
    async fn get_today_orders(&self) -> Result<Vec<Order>>;
    async fn get_open_orders(&self) -> Result<Vec<Order>>;
    async fn cancel_order(&self, order_id: &str) -> Result<()>;
    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>>;
}

#[derive(Debug, Clone)]
pub struct OrderUpdate {
    pub order_id: String,
    pub client_order_id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub status: OrderStatus,
    pub filled_qty: Decimal,
    pub filled_avg_price: Option<Decimal>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait FeatureEngineeringService: Send + Sync {
    fn update(
        &mut self,
        candle: &crate::domain::trading::types::Candle,
    ) -> crate::domain::trading::types::FeatureSet;
}

#[async_trait]
pub trait SectorProvider: Send + Sync {
    async fn get_sector(&self, symbol: &str) -> Result<String>;
}

pub struct Expectancy {
    pub reward_risk_ratio: f64,
    pub win_prob: f64,
    pub expected_value: f64,
}

#[async_trait]
pub trait ExpectancyEvaluator: Send + Sync {
    async fn evaluate(
        &self,
        symbol: &str,
        price: rust_decimal::Decimal,
        regime: &crate::domain::market::market_regime::MarketRegime,
    ) -> Expectancy;
}

#[async_trait]
pub trait NewsDataService: Send + Sync {
    /// Subscribe to a stream of news events
    async fn subscribe_news(&self) -> Result<Receiver<crate::domain::listener::NewsEvent>>;
}
