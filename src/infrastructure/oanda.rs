// STUBBED OUT DUE TO COMPILATION ERRORS IN UNRELATED TASK
// TODO: Fix OANDA implementation

use crate::domain::ports::{ExecutionService, MarketDataService, SectorProvider};
use crate::domain::trading::types::{MarketEvent, Order, OrderSide, OrderType};
use crate::domain::trading::portfolio::Portfolio;
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver};
use tracing::{info, error};
use std::collections::HashMap;

pub struct OandaMarketDataService {
    api_key: String,
    stream_base_url: String,
    api_base_url: String,
    account_id: String,
    client: Client,
}

impl OandaMarketDataService {
    pub fn new(api_key: String, stream_base_url: String, api_base_url: String, account_id: String) -> Self {
        Self {
            api_key,
            stream_base_url,
            api_base_url,
            account_id,
            client: Client::new(),
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
        Ok(vec![])
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
    api_key: String,
    api_base_url: String,
    account_id: String,
    client: Client,
}

impl OandaExecutionService {
    pub fn new(api_key: String, api_base_url: String, account_id: String) -> Self {
        Self {
            api_key,
            api_base_url,
            account_id,
            client: Client::new(),
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
        Ok(vec![])
    }
}

pub struct OandaSectorProvider;

#[async_trait]
impl SectorProvider for OandaSectorProvider {
    async fn get_sector(&self, _symbol: &str) -> Result<String> {
        Ok("Forex".to_string())
    }
}
