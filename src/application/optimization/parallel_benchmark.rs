use rayon::prelude::*;
use crate::application::optimization::simulator::{Simulator, BacktestResult};
use crate::application::agents::analyst::AnalystConfig;
use crate::domain::ports::MarketDataService;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::fee_model::ConstantFeeModel;
use crate::infrastructure::mock::MockExecutionService;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use anyhow::Result;
use rust_decimal::Decimal;
use tokio::sync::RwLock;

/// Result of a single backtest run in a batch
#[derive(Debug, Clone)]
pub struct BatchBacktestResult {
    pub symbol: String,
    pub result: Result<BacktestResult, String>,
}

/// Parallel benchmark runner for multi-symbol backtests
///
/// This runner uses Rayon to execute backtests for multiple symbols concurrently,
/// significantly reducing total execution time on multi-core systems.
///
/// # Example
///
/// ```no_run
/// use rustrade::application::optimization::parallel_benchmark::ParallelBenchmarkRunner;
/// use rustrade::application::agents::analyst::AnalystConfig;
/// use std::sync::Arc;
/// use chrono::Utc;
///
/// # async fn example(market_service: Arc<dyn rustrade::domain::ports::MarketDataService>, config: AnalystConfig) {
/// let runner = ParallelBenchmarkRunner::new(market_service, config);
/// let symbols = vec!["AAPL".to_string(), "TSLA".to_string(), "NVDA".to_string()];
/// let start = Utc::now() - chrono::Duration::days(30);
/// let end = Utc::now();
///
/// let results = runner.run_parallel(symbols, start, end).await;
/// for result in results {
///     match result.result {
///         Ok(backtest) => println!("{}: {:.2}%", result.symbol, backtest.total_return_pct),
///         Err(e) => println!("{}: Error - {}", result.symbol, e),
///     }
/// }
/// # }
/// ```
pub struct ParallelBenchmarkRunner {
    market_service: Arc<dyn MarketDataService>,
    config: AnalystConfig,
}

impl ParallelBenchmarkRunner {
    /// Create a new parallel benchmark runner
    ///
    /// # Arguments
    ///
    /// * `market_service` - Market data service for fetching historical data
    /// * `config` - Analyst configuration to use for all backtests
    pub fn new(
        market_service: Arc<dyn MarketDataService>,
        config: AnalystConfig,
    ) -> Self {
        Self {
            market_service,
            config,
        }
    }

    /// Run backtests for multiple symbols in parallel
    ///
    /// This method uses Rayon's parallel iterator to execute backtests concurrently.
    /// Each symbol gets its own isolated portfolio and execution service to avoid
    /// race conditions.
    ///
    /// # Arguments
    ///
    /// * `symbols` - List of symbols to backtest
    /// * `start` - Start date for the backtest period
    /// * `end` - End date for the backtest period
    ///
    /// # Returns
    ///
    /// A vector of `BatchBacktestResult` containing results for each symbol.
    /// Errors for individual symbols are captured and returned as `Err` variants,
    /// allowing partial results to be collected.
    pub async fn run_parallel(
        &self,
        symbols: Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<BatchBacktestResult> {
        // Get a handle to the current Tokio runtime
        let handle = tokio::runtime::Handle::current();
        
        // Use Rayon's parallel iterator to process symbols concurrently
        symbols.into_par_iter()
            .map(|symbol| {
                let market_service = self.market_service.clone();
                let config = self.config.clone();
                let symbol_clone = symbol.clone(); // Clone before moving into async
                
                // Block on the async task from within the Rayon thread pool
                let result = handle.block_on(async move {
                    Self::run_single(&market_service, &config, &symbol_clone, start, end).await
                });
                
                BatchBacktestResult {
                    symbol: symbol.clone(),
                    result: result.map_err(|e| e.to_string()),
                }
            })
            .collect()
    }

    /// Run a single backtest for one symbol
    ///
    /// This is a helper method that creates isolated resources for each backtest run.
    /// Each run gets its own portfolio and execution service to ensure thread safety.
    async fn run_single(
        market_service: &Arc<dyn MarketDataService>,
        config: &AnalystConfig,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<BacktestResult> {
        // Create a fresh portfolio for this backtest
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::new(100_000, 0); // $100k starting capital
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        
        // Get transaction costs from environment or use defaults
        let slippage_pct = std::env::var("SLIPPAGE_PCT")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.001);
        let commission_per_share = std::env::var("COMMISSION_PER_SHARE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.001);
        
        let slippage = Decimal::try_from(slippage_pct).unwrap_or(Decimal::ZERO);
        let commission = Decimal::try_from(commission_per_share).unwrap_or(Decimal::ZERO);
        let fee_model = Arc::new(ConstantFeeModel::new(commission, slippage));
        
        // Create isolated execution service
        let execution_service = Arc::new(MockExecutionService::with_costs(
            portfolio_lock,
            fee_model,
        ));
        
        // Run the simulation
        let simulator = Simulator::new(
            market_service.clone(),
            execution_service,
            config.clone(),
        );
        
        simulator.run(symbol, start, end).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::{Candle, MarketEvent};
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use tokio::sync::mpsc::Receiver;
    use std::collections::HashMap;

    /// Mock market data service for testing
    struct MockMarketDataService {
        candles: Vec<Candle>,
    }

    #[async_trait]
    impl MarketDataService for MockMarketDataService {
        async fn subscribe(&self, _symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }

        async fn get_historical_bars(
            &self,
            _symbol: &str,
            _start: DateTime<Utc>,
            _end: DateTime<Utc>,
            _timeframe: &str,
        ) -> Result<Vec<Candle>> {
            Ok(self.candles.clone())
        }

        async fn get_prices(&self, _symbols: Vec<String>) -> Result<HashMap<String, Decimal>> {
            let mut prices = HashMap::new();
            prices.insert("TEST".to_string(), dec!(100.0));
            Ok(prices)
        }

        async fn get_top_movers(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_parallel_benchmark_runner_creation() {
        let market_service = Arc::new(MockMarketDataService { candles: vec![] });
        let config = AnalystConfig::default();
        
        let runner = ParallelBenchmarkRunner::new(market_service, config);
        
        // Just verify it compiles and constructs
        let prices = runner.market_service.get_prices(vec!["TEST".to_string()]).await;
        assert!(prices.is_ok());
    }

    #[tokio::test]
    async fn test_batch_backtest_result_error_handling() {
        let result = BatchBacktestResult {
            symbol: "TEST".to_string(),
            result: Err("Test error".to_string()),
        };
        
        assert_eq!(result.symbol, "TEST");
        assert!(result.result.is_err());
    }
}
