use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::optimization::parallel_benchmark::ParallelBenchmarkRunner;
use crate::application::optimization::simulator::{BacktestResult, Simulator};
use crate::config::{Config, StrategyMode};
use crate::domain::risk::risk_appetite::RiskAppetite;
use crate::domain::trading::fee_model::ConstantFeeModel;
use crate::domain::trading::portfolio::Portfolio;
use crate::infrastructure::alpaca::AlpacaMarketDataService;
use crate::infrastructure::mock::MockExecutionService;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct BenchmarkEngine {
    market_service: Arc<AlpacaMarketDataService>,
    base_config: Config,
}

impl BenchmarkEngine {
    pub async fn new() -> Self {
        // Load env
        if dotenv::from_filename(".env.benchmark").is_err() {
            dotenv::dotenv().ok();
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
        let slippage = Decimal::from_f64(0.001).expect("0.001 is a valid f64 for Decimal");
        let commission = Decimal::from_f64(0.001).expect("0.001 is a valid f64 for Decimal");
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
