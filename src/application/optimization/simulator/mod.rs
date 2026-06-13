pub mod engine;
pub mod helpers;
pub mod result;

use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::ports::{ExecutionService, MarketDataService};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub use helpers::InMemoryCandleRepository;
pub use result::BacktestResult;

pub struct Simulator {
    pub(crate) market_data: Arc<dyn MarketDataService>,
    pub(crate) execution_service: Arc<dyn ExecutionService>,
    pub(crate) config: AnalystConfig,
}

impl Simulator {
    pub fn new(
        market_data: Arc<dyn MarketDataService>,
        execution_service: Arc<dyn ExecutionService>,
        config: AnalystConfig,
    ) -> Self {
        Self {
            market_data,
            execution_service,
            config,
        }
    }

    pub async fn run(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
    ) -> Result<BacktestResult> {
        let bars = self
            .market_data
            .get_historical_bars(symbol, start, end, timeframe)
            .await
            .context("Failed to fetch historical bars")?;
        self.run_with_bars(symbol, &bars, start, end, None).await
    }

    pub async fn run_multi_asset(
        &self,
        symbols: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
    ) -> Result<BacktestResult> {
        if symbols.is_empty() {
            anyhow::bail!("Simulator: no symbols provided for run_multi_asset");
        }

        // Fetch historical bars for all symbols
        let mut futures = Vec::new();
        for symbol in symbols {
            let market = self.market_data.clone();
            let symbol_clone = symbol.clone();
            let tf = timeframe.to_string();
            futures.push(tokio::spawn(async move {
                market
                    .get_historical_bars(&symbol_clone, start, end, &tf)
                    .await
            }));
        }

        let results = futures::future::join_all(futures).await;
        let mut all_candles = Vec::new();
        for (idx, res) in results.into_iter().enumerate() {
            let symbol = &symbols[idx];
            let bars = res
                .context(format!("Failed to join thread for {}", symbol))?
                .context(format!("Failed to fetch historical bars for {}", symbol))?;
            all_candles.extend(bars);
        }

        if all_candles.is_empty() {
            anyhow::bail!("Simulator: no bars fetched for any symbol");
        }

        // Sort chronologically by timestamp
        all_candles.sort_by_key(|c| c.timestamp);

        self.run_with_multi_bars(symbols, &all_candles, start, end, None)
            .await
    }
}
