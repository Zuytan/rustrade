use crate::application::optimization::simulator::Simulator;
use crate::domain::performance::stats::Stats;
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::types::Candle;
use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Mutex;

impl Simulator {
    /// Calculate alpha and beta using linear regression
    /// Returns (alpha, beta, correlation)
    /// Formula: strategy_return = alpha + beta * benchmark_return + error
    pub(crate) fn calculate_alpha_beta(
        strategy_returns: &[Decimal],
        benchmark_returns: &[Decimal],
    ) -> (f64, f64, f64) {
        let (a, b, c) = Stats::alpha_beta(strategy_returns, benchmark_returns);
        (
            a.to_f64().unwrap_or(0.0),
            b.to_f64().unwrap_or(0.0),
            c.to_f64().unwrap_or(0.0),
        )
    }
}

// Helper Repository for Simulator
pub struct InMemoryCandleRepository {
    candles: Mutex<Vec<Candle>>,
}

impl InMemoryCandleRepository {
    pub fn new(candles: Vec<Candle>) -> Self {
        Self {
            candles: Mutex::new(candles),
        }
    }
}

#[async_trait]
impl CandleRepository for InMemoryCandleRepository {
    async fn save(&self, _candle: &Candle) -> Result<()> {
        Ok(())
    }

    async fn get_range(&self, symbol: &str, start_ts: i64, end_ts: i64) -> Result<Vec<Candle>> {
        let candles = self
            .candles
            .lock()
            .expect("InMemoryCandleRepository mutex poisoned - concurrent panic");
        Ok(candles
            .iter()
            .filter(|c| c.symbol == symbol && c.timestamp >= start_ts && c.timestamp <= end_ts)
            .cloned()
            .collect())
    }

    async fn get_latest_timestamp(&self, symbol: &str) -> Result<Option<i64>> {
        let candles = self
            .candles
            .lock()
            .expect("InMemoryCandleRepository mutex poisoned - concurrent panic");
        Ok(candles
            .iter()
            .rfind(|c| c.symbol == symbol)
            .map(|c| c.timestamp))
    }

    async fn count_candles(&self, _symbol: &str, _start_ts: i64, _end_ts: i64) -> Result<usize> {
        Ok(0)
    }

    async fn prune(&self, _days_retention: i64) -> Result<u64> {
        Ok(0)
    }
}
