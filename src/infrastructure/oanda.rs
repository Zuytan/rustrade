// STUBBED OUT DUE TO COMPILATION ERRORS IN UNRELATED TASK
// TODO: Fix OANDA implementation

use crate::domain::ports::OrderUpdate;
use crate::domain::ports::{ExecutionService, MarketDataService, SectorProvider};
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{MarketEvent, Order};
use anyhow::Result;
use async_trait::async_trait;

use reqwest::Client;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, Receiver};

pub struct OandaMarketDataService {
    _api_key: String,
    _stream_base_url: String,
    _api_base_url: String,
    _account_id: String,
    _client: Client,
}

impl OandaMarketDataService {
    pub fn new(
        api_key: String,
        stream_base_url: String,
        api_base_url: String,
        account_id: String,
    ) -> Self {
        Self {
            _api_key: api_key,
            _stream_base_url: stream_base_url,
            _api_base_url: api_base_url,
            _account_id: account_id,
            _client: Client::new(),
        }
    }
}

#[async_trait]
impl MarketDataService for OandaMarketDataService {
    async fn subscribe(&self, _symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        let (_, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        // OANDA doesn't have a "movers" endpoint like stock screeners.
        // Return a fixed list of majors/minors for now.
        Ok(vec![
            "EUR_USD".to_string(),
            "GBP_USD".to_string(),
            "USD_JPY".to_string(),
            "AUD_USD".to_string(),
            "USD_CAD".to_string(),
        ])
    }

    async fn get_prices(
        &self,
        _symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, rust_decimal::Decimal>> {
        Ok(std::collections::HashMap::new())
    }

    async fn get_historical_bars(
        &self,
        _symbol: &str,
        _start: chrono::DateTime<chrono::Utc>,
        _end: chrono::DateTime<chrono::Utc>,
        _timeframe: &str,
    ) -> Result<Vec<crate::domain::trading::types::Candle>> {
        Ok(vec![])
    }
}

pub struct OandaExecutionService {
    _api_key: String,
    _api_base_url: String,
    _account_id: String,
    _client: Client,
}

impl OandaExecutionService {
    pub fn new(api_key: String, api_base_url: String, account_id: String) -> Self {
        Self {
            _api_key: api_key,
            _api_base_url: api_base_url,
            _account_id: account_id,
            _client: Client::new(),
        }
    }
}

#[async_trait]
impl ExecutionService for OandaExecutionService {
    async fn execute(&self, _order: Order) -> Result<()> {
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        Ok(Portfolio::new())
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        // Not implemented for now
        Ok(Vec::new())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        // Oanda implementation pending, return empty for now
        Ok(vec![])
    }

    async fn cancel_order(&self, _order_id: &str) -> Result<()> {
        // Oanda implementation pending
        Ok(())
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        anyhow::bail!("Oanda order updates not implemented")
    }
}

pub struct OandaSectorProvider;

#[async_trait]
impl SectorProvider for OandaSectorProvider {
    async fn get_sector(&self, _symbol: &str) -> Result<String> {
        Ok("Forex".to_string())
    }
}
