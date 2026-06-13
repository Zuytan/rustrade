use crate::domain::market::strategy_config::StrategyDefinition;
use crate::domain::repositories::{CandleRepository, StrategyRepository, TradeRepository};
use crate::domain::trading::types::{Candle, Order, OrderStatus};
use anyhow::Result;
use async_trait::async_trait;

pub struct NullTradeRepository;

#[async_trait]
impl TradeRepository for NullTradeRepository {
    async fn save(&self, _trade: &Order) -> Result<()> {
        Ok(())
    }
    async fn find_by_symbol(&self, _symbol: &str) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn find_by_status(&self, _status: OrderStatus) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn find_recent(&self, _limit: usize) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn get_all(&self) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn count(&self) -> Result<usize> {
        Ok(0)
    }
}

pub struct NullCandleRepository;

#[async_trait]
impl CandleRepository for NullCandleRepository {
    async fn save(&self, _candle: &Candle) -> Result<()> {
        Ok(())
    }
    async fn get_range(&self, _symbol: &str, _start_ts: i64, _end_ts: i64) -> Result<Vec<Candle>> {
        Ok(vec![])
    }
    async fn get_latest_timestamp(&self, _symbol: &str) -> Result<Option<i64>> {
        Ok(None)
    }
    async fn count_candles(&self, _symbol: &str, _start_ts: i64, _end_ts: i64) -> Result<usize> {
        Ok(0)
    }
    async fn prune(&self, _days_retention: i64) -> Result<u64> {
        Ok(0)
    }
}

pub struct NullStrategyRepository;

#[async_trait]
impl StrategyRepository for NullStrategyRepository {
    async fn save(&self, _config: &StrategyDefinition) -> Result<()> {
        Ok(())
    }
    async fn find_by_symbol(&self, _symbol: &str) -> Result<Option<StrategyDefinition>> {
        Ok(None)
    }
    async fn get_all_active(&self) -> Result<Vec<StrategyDefinition>> {
        Ok(vec![])
    }
}
