use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::optimization::parallel_benchmark::ParallelBenchmarkRunner;
use crate::application::optimization::simulator::{BacktestResult, Simulator};
use crate::config::{Config, StrategyMode};
use crate::domain::performance::metrics::PerformanceMetrics;
use crate::domain::risk::risk_appetite::RiskAppetite;
use crate::domain::trading::fee_model::ConstantFeeModel;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Order, OrderSide, Trade};
use crate::infrastructure::alpaca::AlpacaMarketDataService;
use crate::infrastructure::mock::MockExecutionService;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use std::env;
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

pub struct BenchmarkEngine {
    market_service: Arc<AlpacaMarketDataService>,
    base_config: Config,
}

impl BenchmarkEngine {
    pub async fn new() -> Self {
        // Load env
        if dotenvy::from_filename(".env.benchmark").is_err() {
            dotenvy::dotenv().ok();
        }

        let api_key = env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY must be set");
        let api_secret = env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY must be set");
        let data_url =
            env::var("ALPACA_DATA_URL").unwrap_or("https://data.alpaca.markets".to_string());
        let ws_url = env::var("ALPACA_WS_URL")
            .unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());

        let base_config = Config::from_env().unwrap_or_else(|_| {
            eprintln!("Warning: Failed to load config from env, using defaults");
            // In a real app we might want to fail hard here or return Result
            // For now constructing a default or panicking is what the original did
            panic!("Failed to load config");
        });

        let market_service = Arc::new(
            AlpacaMarketDataService::builder()
                .api_key(api_key)
                .api_secret(api_secret)
                .data_base_url(data_url)
                .ws_url(ws_url)
                // Use existing config val or 0.0 for benchmark strictness
                .min_volume_threshold(0.0)
                .asset_class(crate::config::AssetClass::Stock)
                .build(),
        );

        Self {
            market_service,
            base_config,
        }
    }

    pub async fn run_single(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        risk_score: Option<u8>,
    ) -> anyhow::Result<BacktestResult> {
        let mut app_config = self.base_config.clone();
        app_config.strategy_mode = strategy;

        if let Some(score) = risk_score {
            let risk_appetite =
                RiskAppetite::new(score).expect("risk_score validated within 1-9 range");
            app_config.risk_appetite = Some(risk_appetite);
        }

        let mut config: AnalystConfig = app_config.clone().into();

        // Ensure risk appetite is applied if present in app_config
        if let Some(ra) = &app_config.risk_appetite {
            config.apply_risk_appetite(ra);
        }

        self.execute_simulation(symbol, start, end, config).await
    }

    pub async fn run_parallel(
        &self,
        symbols: Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
    ) -> Vec<crate::application::optimization::parallel_benchmark::BatchBacktestResult> {
        let mut app_config = self.base_config.clone();
        app_config.strategy_mode = strategy;
        let config: AnalystConfig = app_config.into();

        let runner = ParallelBenchmarkRunner::new(self.market_service.clone(), config);
        runner.run_parallel(symbols, start, end).await
    }

    pub async fn get_historical_movers(
        &self,
        date: chrono::NaiveDate,
        universe: &[String],
    ) -> anyhow::Result<Vec<String>> {
        self.market_service
            .get_historical_movers(date, universe)
            .await
    }

    pub async fn get_top_movers(&self) -> anyhow::Result<Vec<String>> {
        use crate::domain::ports::MarketDataService;
        self.market_service.get_top_movers().await
    }

    /// Walk-forward backtesting: split [start, end] into train (e.g. 70%) and test (30%),
    /// run backtest on the test window only, and return out-of-sample Sharpe to detect overfitting.
    pub async fn run_walk_forward(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        strategy: StrategyMode,
        train_ratio: f64,
        risk_score: Option<u8>,
    ) -> anyhow::Result<WalkForwardResult> {
        let total_secs = (end - start).num_seconds();
        let train_secs = (total_secs as f64 * train_ratio) as i64;
        let test_start = start + Duration::seconds(train_secs);

        if test_start >= end {
            anyhow::bail!(
                "Walk-forward: test window empty (train_ratio too high or window too short)"
            );
        }

        let mut app_config = self.base_config.clone();
        app_config.strategy_mode = strategy;
        if let Some(score) = risk_score {
            let risk_appetite =
                RiskAppetite::new(score).expect("risk_score validated within 1-9 range");
            app_config.risk_appetite = Some(risk_appetite);
        }
        let mut config: AnalystConfig = app_config.clone().into();
        if let Some(ra) = &app_config.risk_appetite {
            config.apply_risk_appetite(ra);
        }

        let result = self
            .execute_simulation(symbol, test_start, end, config)
            .await?;

        let trades = orders_to_trades(&result.trades);
        let metrics = PerformanceMetrics::calculate_time_series_metrics(
            &trades,
            &result.daily_closes,
            result.initial_equity,
        );

        Ok(WalkForwardResult {
            test_start,
            test_end: end,
            oos_sharpe_ratio: metrics.sharpe_ratio,
            oos_return_pct: result.total_return_pct,
            oos_trade_count: trades.len(),
        })
    }

    async fn execute_simulation(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        config: AnalystConfig,
    ) -> anyhow::Result<BacktestResult> {
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));

        // Use standard benchmark costs
        let slippage = Decimal::from_f64_retain(0.001).expect("0.001 is a valid f64 for Decimal");
        let commission = Decimal::from_f64_retain(0.001).expect("0.001 is a valid f64 for Decimal");
        let fee_model = Arc::new(ConstantFeeModel::new(commission, slippage));

        // Check for simulation mode (Step 2: High-Fidelity Simulation)
        let execution_service = if self.base_config.simulation_enabled {
            use crate::infrastructure::simulation::latency_model::NetworkLatency;
            use crate::infrastructure::simulation::slippage_model::VolatilitySlippage;

            let latency_model = Arc::new(NetworkLatency::new(
                self.base_config.simulation_latency_base_ms,
                self.base_config.simulation_latency_jitter_ms,
            ));
            let slippage_model = Arc::new(VolatilitySlippage::new(
                self.base_config.simulation_slippage_volatility,
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

        simulator.run(symbol, start, end).await
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
                    });
                    open_position = None;
                }
            }
        }
    }
    trades
}
