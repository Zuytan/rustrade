//! Optimization engine for parameter search.
//!
//! Uses a genetic algorithm by default (single period, parallel evaluation).
//! Bounds are derived from ParameterGrid when provided.

use crate::application::optimization::optimizer::{
    GeneticOptimizer, OptimizationResult, ParameterGrid,
};
use crate::config::{AssetClass, Config, StrategyMode};
use crate::domain::ports::ExecutionService;
use crate::domain::trading::fee_model::ConstantFeeModel;
use crate::domain::trading::portfolio::Portfolio;
use crate::infrastructure::alpaca::AlpacaMarketDataService;
use crate::infrastructure::mock::MockExecutionService;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// High-level optimization engine that encapsulates service setup and execution.
pub struct OptimizeEngine {
    market_service: Arc<AlpacaMarketDataService>,
    base_config: Config,
}

impl OptimizeEngine {
    /// Creates a new OptimizeEngine, loading configuration from environment.
    pub fn new() -> Result<Self> {
        // Load env
        dotenvy::dotenv().ok();

        let api_key = env::var("ALPACA_API_KEY").context("ALPACA_API_KEY must be set")?;
        let api_secret = env::var("ALPACA_SECRET_KEY").context("ALPACA_SECRET_KEY must be set")?;
        let data_url = env::var("ALPACA_DATA_URL")
            .unwrap_or_else(|_| "https://data.alpaca.markets".to_string());
        let api_base_url = env::var("ALPACA_BASE_URL")
            .unwrap_or_else(|_| "https://paper-api.alpaca.markets".to_string());
        let ws_url = env::var("ALPACA_WS_URL")
            .unwrap_or_else(|_| "wss://stream.data.alpaca.markets/v2/iex".to_string());
        let asset_class_str = env::var("ASSET_CLASS").unwrap_or_else(|_| "stock".to_string());
        let asset_class = AssetClass::from_str(&asset_class_str).unwrap_or(AssetClass::Stock);

        let base_config = Config::from_env().context("Failed to load config from environment")?;

        let market_service = Arc::new(
            AlpacaMarketDataService::builder()
                .api_key(api_key)
                .api_secret(api_secret)
                .data_base_url(data_url)
                .api_base_url(api_base_url)
                .ws_url(ws_url)
                .min_volume_threshold(dec!(10000.0).to_f64().unwrap_or(10000.0))
                .asset_class(asset_class)
                .candle_repository(None) // No caching needed for optimization
                .build(),
        );

        Ok(Self {
            market_service,
            base_config,
        })
    }

    /// Runs parameter optimization for a single symbol using a genetic algorithm.
    /// Bounds are derived from parameter_grid; population/generations control the search.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_grid_search(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        parameter_grid: ParameterGrid,
        _train_ratio: f64,
        risk_score: Option<u8>,
    ) -> Result<Vec<OptimizationResult>> {
        self.run_genetic_optimization(
            symbol,
            start,
            end,
            strategy,
            parameter_grid,
            None,
            None,
            None,
            None,
            risk_score,
        )
        .await
    }

    /// Runs genetic optimization with optional tuning (population, generations, mutation_rate, timeframe, risk_score).
    #[allow(clippy::too_many_arguments)]
    pub async fn run_genetic_optimization(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        parameter_grid: ParameterGrid,
        population_size: Option<usize>,
        generations: Option<usize>,
        mutation_rate: Option<f64>,
        timeframe: Option<String>,
        risk_score: Option<u8>,
    ) -> Result<Vec<OptimizationResult>> {
        let execution_service_factory = self.create_execution_factory();
        let bounds = parameter_grid.gene_bounds();
        let population_size = population_size.unwrap_or(24);
        let generations = generations.unwrap_or(15);
        let mutation_rate = mutation_rate.unwrap_or(0.15);
        let timeframe = timeframe.as_deref().unwrap_or("1Min");

        let optimizer = GeneticOptimizer::new(
            self.market_service.clone(),
            execution_service_factory,
            bounds,
            strategy,
            self.base_config.min_profit_ratio,
            population_size,
            generations,
            mutation_rate,
            risk_score,
        );

        optimizer
            .run_optimization(symbol, start, end, timeframe)
            .await
    }

    /// Runs optimization on multiple symbols sequentially.
    pub async fn run_batch(
        &self,
        symbols: Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        parameter_grid: ParameterGrid,
        train_ratio: f64,
    ) -> Vec<(String, Result<Vec<OptimizationResult>>)> {
        let mut results = Vec::new();

        for symbol in symbols {
            info!("Running optimization for {}", symbol);
            let result = self
                .run_grid_search(
                    &symbol,
                    start,
                    end,
                    strategy,
                    parameter_grid.clone(),
                    train_ratio,
                    None, // risk_score: batch does not expose risk_score yet
                )
                .await;
            results.push((symbol, result));
        }

        results
    }

    /// Ranks results and returns the top N configurations.
    pub fn rank_results(
        &self,
        results: Vec<OptimizationResult>,
        top_n: usize,
    ) -> Vec<OptimizationResult> {
        let mut sorted = results;
        sorted.sort_by(|a, b| {
            b.objective_score
                .partial_cmp(&a.objective_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.truncate(top_n);
        sorted
    }

    /// Creates a new execution service factory for each optimization run.
    fn create_execution_factory(&self) -> Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync> {
        Arc::new(move || {
            let mut portfolio = Portfolio::new();
            portfolio.cash = Decimal::new(100000, 0);
            let portfolio_lock = Arc::new(RwLock::new(portfolio));

            let slippage_pct = env::var("SLIPPAGE_PCT")
                .unwrap_or_else(|_| "0.001".to_string())
                .parse::<f64>()
                .unwrap_or(0.001);
            let commission_per_share = env::var("COMMISSION_PER_SHARE")
                .unwrap_or_else(|_| "0.001".to_string())
                .parse::<f64>()
                .unwrap_or(0.001);

            let slippage = Decimal::from_f64_retain(slippage_pct).unwrap_or(Decimal::ZERO);
            let commission =
                Decimal::from_f64_retain(commission_per_share).unwrap_or(Decimal::ZERO);
            let fee_model = Arc::new(ConstantFeeModel::new(commission, slippage));

            Arc::new(MockExecutionService::with_costs(portfolio_lock, fee_model))
        })
    }
}

use std::str::FromStr;
