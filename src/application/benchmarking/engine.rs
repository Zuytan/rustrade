use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::optimization::parallel_benchmark::ParallelBenchmarkRunner;
use crate::application::optimization::simulator::{BacktestResult, Simulator};
use crate::config::{Config, StrategyMode};
use crate::domain::performance::metrics::PerformanceMetrics;
use crate::domain::ports::MarketDataService;
use crate::domain::risk::risk_appetite::RiskAppetite;
use crate::domain::trading::fee_model::ConstantFeeModel;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Order, OrderSide, Trade};
use crate::infrastructure::mock::MockExecutionService;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Result of walk-forward backtesting: out-of-sample metrics to detect overfitting.
#[derive(Debug, Clone)]
pub struct WalkForwardResult {
    pub test_start: DateTime<Utc>,
    pub test_end: DateTime<Utc>,
    pub oos_sharpe_ratio: f64,
    pub oos_return_pct: Decimal,
    pub oos_trade_count: usize,
}

/// Anchored walk-forward: multiple expanding train windows with consecutive test windows.
/// `oos_sharpe_std` is the standard deviation of OOS Sharpe across folds (stability metric).
#[derive(Debug, Clone)]
pub struct AnchoredWalkForwardResult {
    pub results: Vec<WalkForwardResult>,
    pub oos_sharpe_mean: f64,
    pub oos_sharpe_std: f64,
}

pub struct BenchmarkEngine {
    market_service: Arc<dyn MarketDataService>,
    pub base_config: Config,
}

impl BenchmarkEngine {
    /// Creates a new BenchmarkEngine with an injected MarketDataService.
    ///
    /// This constructor allows using any market data source (Mock, Binance, etc.)
    /// and is the preferred way to create a BenchmarkEngine for testing or
    /// non-Alpaca environments.
    pub fn new(market_service: Arc<dyn MarketDataService>, config: Config) -> Self {
        Self {
            market_service,
            base_config: config,
        }
    }

    pub async fn run_single(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        risk_score: Option<u8>,
        timeframe: &str,
    ) -> anyhow::Result<BacktestResult> {
        let mut app_config = self.base_config.clone();
        app_config.strategy.strategy_mode = strategy;

        if let Some(score) = risk_score {
            app_config.strategy.risk_appetite_score = Some(score);
        }

        let mut config: AnalystConfig = app_config.clone().into();

        // Ensure risk appetite is applied if present
        if let Some(score) = app_config.strategy.risk_appetite_score
            && let Ok(appetite) = RiskAppetite::new(score)
        {
            config.apply_risk_appetite(&appetite);
        }

        self.execute_simulation(symbol, start, end, config, timeframe)
            .await
    }

    /// Runs a single backtest with the given AnalystConfig (e.g. from optimization results).
    /// Use this to benchmark a parameter set produced by the optimize binary.
    pub async fn run_single_with_config(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        config: AnalystConfig,
        timeframe: &str,
    ) -> anyhow::Result<BacktestResult> {
        self.execute_simulation(symbol, start, end, config, timeframe)
            .await
    }

    pub async fn run_multi_asset(
        &self,
        symbols: Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        risk_score: Option<u8>,
        timeframe: &str,
    ) -> anyhow::Result<BacktestResult> {
        let mut app_config = self.base_config.clone();
        app_config.strategy.strategy_mode = strategy;

        if let Some(score) = risk_score {
            app_config.strategy.risk_appetite_score = Some(score);
        }

        let mut config: AnalystConfig = app_config.clone().into();

        if let Some(score) = app_config.strategy.risk_appetite_score
            && let Ok(appetite) = RiskAppetite::new(score)
        {
            config.apply_risk_appetite(&appetite);
        }

        self.execute_simulation_multi(&symbols, start, end, config, timeframe)
            .await
    }

    pub async fn run_parallel(
        &self,
        symbols: Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        timeframe: &str,
    ) -> Vec<crate::application::optimization::parallel_benchmark::BatchBacktestResult> {
        let mut app_config = self.base_config.clone();
        app_config.strategy.strategy_mode = strategy;
        let config: AnalystConfig = app_config.into();

        let runner = ParallelBenchmarkRunner::new(self.market_service.clone(), config);
        runner
            .run_parallel(symbols, start, end, timeframe.to_string())
            .await
    }

    /// Walk-forward backtesting: single split train (e.g. 70%) / test (30%), run backtest on test only.
    pub async fn run_walk_forward(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        train_ratio: f64,
        risk_score: Option<u8>,
    ) -> anyhow::Result<WalkForwardResult> {
        let anchored = self
            .run_anchored_walk_forward(
                symbol,
                start,
                end,
                strategy,
                train_ratio,
                1,
                risk_score,
                "1Min",
            )
            .await?;
        anchored
            .results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Anchored walk-forward returned no folds"))
    }

    /// Anchored walk-forward: expanding train window, multiple consecutive test windows.
    /// Returns OOS results per fold and stability (std of OOS Sharpe).
    #[allow(clippy::too_many_arguments)]
    pub async fn run_anchored_walk_forward(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        train_ratio: f64,
        num_folds: u32,
        risk_score: Option<u8>,
        timeframe: &str,
    ) -> anyhow::Result<AnchoredWalkForwardResult> {
        let total_secs = (end - start).num_seconds();
        let train_secs = (total_secs as f64 * train_ratio) as i64;
        let test_region_secs = total_secs - train_secs;
        if test_region_secs <= 0 || num_folds == 0 {
            anyhow::bail!(
                "Walk-forward: test region empty or num_folds=0 (train_ratio too high or window too short)"
            );
        }
        let test_window_secs = test_region_secs / num_folds as i64;
        if test_window_secs <= 0 {
            anyhow::bail!(
                "Walk-forward: test window too short for {} folds",
                num_folds
            );
        }

        let mut app_config = self.base_config.clone();
        app_config.strategy.strategy_mode = strategy;
        if let Some(score) = risk_score {
            app_config.strategy.risk_appetite_score = Some(score);
        }
        let mut config: AnalystConfig = app_config.clone().into();
        if let Some(score) = app_config.strategy.risk_appetite_score
            && let Ok(appetite) = RiskAppetite::new(score)
        {
            config.apply_risk_appetite(&appetite);
        }

        let mut results = Vec::with_capacity(num_folds as usize);
        for i in 0..num_folds {
            let test_start = start + Duration::seconds(train_secs + i as i64 * test_window_secs);
            let test_end = (test_start + Duration::seconds(test_window_secs)).min(end);

            if test_start >= end || test_end <= test_start {
                continue;
            }

            let result = self
                .execute_simulation(symbol, test_start, test_end, config.clone(), timeframe)
                .await?;

            let trades = orders_to_trades(&result.trades);
            let metrics = PerformanceMetrics::calculate_time_series_metrics(
                &trades,
                &result.daily_closes,
                result.initial_equity,
            );

            results.push(WalkForwardResult {
                test_start,
                test_end,
                oos_sharpe_ratio: metrics.sharpe_ratio,
                oos_return_pct: result.total_return_pct,
                oos_trade_count: trades.len(),
            });
        }

        let n = results.len() as f64;
        let mean = if n > 0.0 {
            results.iter().map(|r| r.oos_sharpe_ratio).sum::<f64>() / n
        } else {
            0.0
        };
        let variance = if n > 1.0 {
            results
                .iter()
                .map(|r| (r.oos_sharpe_ratio - mean).powi(2))
                .sum::<f64>()
                / (n - 1.0)
        } else {
            0.0
        };
        let oos_sharpe_std = variance.sqrt();

        Ok(AnchoredWalkForwardResult {
            results,
            oos_sharpe_mean: mean,
            oos_sharpe_std,
        })
    }

    async fn execute_simulation(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        config: AnalystConfig,
        timeframe: &str,
    ) -> anyhow::Result<BacktestResult> {
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));

        // Use standard benchmark costs
        let slippage = Decimal::from_f64_retain(0.001).expect("0.001 is a valid f64 for Decimal");
        let commission = Decimal::from_f64_retain(0.001).expect("0.001 is a valid f64 for Decimal");
        let fee_model = Arc::new(ConstantFeeModel::new(commission, slippage));

        // Check for simulation mode (Step 2: High-Fidelity Simulation)
        let execution_service = if self.base_config.simulation.enabled {
            use crate::infrastructure::simulation::latency_model::NetworkLatency;
            use crate::infrastructure::simulation::slippage_model::VolatilitySlippage;

            let latency_model = Arc::new(NetworkLatency::new(
                self.base_config.simulation.latency_base_ms,
                self.base_config.simulation.latency_jitter_ms,
            ));
            let slippage_model = Arc::new(VolatilitySlippage::new(
                self.base_config.simulation.slippage_volatility,
            ));

            Arc::new(MockExecutionService::with_simulation_models(
                portfolio_lock.clone(),
                fee_model,
                latency_model,
                slippage_model,
            ))
        } else {
            Arc::new(MockExecutionService::with_costs(
                portfolio_lock.clone(),
                fee_model,
            ))
        };

        let simulator = Simulator::new(self.market_service.clone(), execution_service, config);

        simulator.run(symbol, start, end, timeframe).await
    }

    async fn execute_simulation_multi(
        &self,
        symbols: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        config: AnalystConfig,
        timeframe: &str,
    ) -> anyhow::Result<BacktestResult> {
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));

        // Use standard benchmark costs
        let slippage = Decimal::from_f64_retain(0.001).expect("0.001 is a valid f64 for Decimal");
        let commission = Decimal::from_f64_retain(0.001).expect("0.001 is a valid f64 for Decimal");
        let fee_model = Arc::new(ConstantFeeModel::new(commission, slippage));

        // Check for simulation mode (Step 2: High-Fidelity Simulation)
        let execution_service = if self.base_config.simulation.enabled {
            use crate::infrastructure::simulation::latency_model::NetworkLatency;
            use crate::infrastructure::simulation::slippage_model::VolatilitySlippage;

            let latency_model = Arc::new(NetworkLatency::new(
                self.base_config.simulation.latency_base_ms,
                self.base_config.simulation.latency_jitter_ms,
            ));
            let slippage_model = Arc::new(VolatilitySlippage::new(
                self.base_config.simulation.slippage_volatility,
            ));

            Arc::new(MockExecutionService::with_simulation_models(
                portfolio_lock.clone(),
                fee_model,
                latency_model,
                slippage_model,
            ))
        } else {
            Arc::new(MockExecutionService::with_costs(
                portfolio_lock.clone(),
                fee_model,
            ))
        };

        let simulator = Simulator::new(self.market_service.clone(), execution_service, config);

        simulator
            .run_multi_asset(symbols, start, end, timeframe)
            .await
    }
}

/// Convert backtest orders (Buy/Sell pairs) into Trade records for metrics.
fn orders_to_trades(orders: &[Order]) -> Vec<Trade> {
    let mut trades = Vec::new();
    let mut open_position: Option<&Order> = None;
    for order in orders {
        match order.side {
            OrderSide::Buy => open_position = Some(order),
            OrderSide::Sell => {
                if let Some(buy) = open_position {
                    let pnl = (order.price - buy.price) * order.quantity;
                    trades.push(Trade {
                        id: order.id.clone(),
                        symbol: order.symbol.clone(),
                        side: OrderSide::Buy,
                        entry_price: buy.price,
                        exit_price: Some(order.price),
                        quantity: order.quantity,
                        pnl,
                        entry_timestamp: buy.timestamp,
                        exit_timestamp: Some(order.timestamp),
                        strategy_used: None,
                        regime_detected: None,
                        entry_reason: None,
                        exit_reason: None,
                        slippage: None,
                        fees: rust_decimal::Decimal::ZERO,
                    });
                    open_position = None;
                }
            }
        }
    }
    trades
}
