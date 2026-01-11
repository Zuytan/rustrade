//! Quick Verification Benchmark
//! Tests the key improvements from benchmark optimization:
//! 1. Risk-2/5 now generating trades (signal sensitivity)
//! 2. Breakout strategy now active (tuned parameters)
//!
//! Uses same approach as benchmark_matrix.rs for proper comparison.

use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use rustrade::application::agents::analyst::AnalystConfig;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::config::StrategyMode;
use rustrade::domain::risk::risk_appetite::RiskAppetite;
use rustrade::domain::trading::fee_model::ConstantFeeModel;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;
use rustrade::infrastructure::mock::MockExecutionService;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("benchmark_verify=info".parse().unwrap()),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    // 2. Load Env
    if dotenv::from_filename(".env.benchmark").is_err() {
        dotenv::dotenv().ok();
    }

    // 3. Load base config (same as benchmark_matrix)
    let base_config = rustrade::config::Config::from_env().unwrap_or_else(|_| {
        panic!("Failed to load base config");
    });

    // 4. Use 2024 H1 Surge window (same as original benchmark)
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 14, 30, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 6, 30, 21, 0, 0).unwrap();

    let symbol = "NVDA"; // NVDA showed trades in original benchmark

    // Test these specific scenarios to verify fixes
    let test_cases = vec![
        (StrategyMode::Standard, 2, "Risk-2 Standard (was 0 trades)"),
        (StrategyMode::Standard, 5, "Risk-5 Standard (was 0 trades)"),
        (StrategyMode::Standard, 8, "Risk-8 Standard (Expect Trades)"),
        (StrategyMode::Breakout, 8, "Risk-8 Breakout (was 0 trades)"),
        (StrategyMode::TrendRiding, 8, "Risk-8 TrendRiding (control)"),
        (StrategyMode::Advanced, 8, "Risk-8 Advanced (control)"),
    ];

    // 5. Initialize Market Service
    let api_key = env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY must be set");
    let api_secret = env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY must be set");
    let data_url = env::var("ALPACA_DATA_URL").unwrap_or("https://data.alpaca.markets".to_string());
    let ws_url =
        env::var("ALPACA_WS_URL").unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());

    let market_service = Arc::new(
        AlpacaMarketDataService::builder()
            .api_key(api_key)
            .api_secret(api_secret)
            .data_base_url(data_url)
            .ws_url(ws_url)
            .min_volume_threshold(0.0)
            .asset_class(rustrade::config::AssetClass::Stock)
            .build(),
    );

    println!("\n{}", "=".repeat(80));
    println!("🔬 QUICK VERIFICATION BENCHMARK");
    println!(
        "Window: 2024 H1 Surge | Symbol: {} | Testing {} scenarios",
        symbol,
        test_cases.len()
    );
    println!("{}", "=".repeat(80));

    for (strategy, risk_score, description) in test_cases {
        // Create config exactly like benchmark_matrix.rs does
        let mut app_config = base_config.clone();
        app_config.strategy_mode = strategy;
        let risk_appetite = RiskAppetite::new(risk_score).unwrap();
        app_config.risk_appetite = Some(risk_appetite);

        // Convert to AnalystConfig and apply risk appetite (same as benchmark_matrix)
        let mut config: AnalystConfig = app_config.into();
        config.apply_risk_appetite(&RiskAppetite::new(risk_score).unwrap());

        println!(
            "   [Config] Strategy: {:?}, Trend SMA Period: {}",
            strategy, config.trend_sma_period
        );

        // Create fresh portfolio
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));

        // Use same fee model as benchmark_matrix
        let slippage = Decimal::from_f64(0.001).unwrap();
        let commission = Decimal::from_f64(0.001).unwrap();
        let fee_model = Arc::new(ConstantFeeModel::new(commission, slippage));

        let exec_service = Arc::new(MockExecutionService::with_costs(portfolio_lock, fee_model));

        let simulator = Simulator::new(market_service.clone(), exec_service, config);

        match simulator.run(symbol, start, end).await {
            Ok(result) => {
                let trade_count = result.trades.len();
                let status = if trade_count > 0 {
                    "✅ PASS"
                } else {
                    "❌ FAIL"
                };
                println!(
                    "{} | {:?} Risk-{} | Trades: {} | Return: {:.2}%",
                    status, strategy, risk_score, trade_count, result.total_return_pct
                );
                println!("   └─ {}", description);
            }
            Err(e) => {
                println!("❌ ERROR | {:?} Risk-{} | {}", strategy, risk_score, e);
            }
        }
    }

    println!("\n{}", "=".repeat(80));
    println!("🏁 VERIFICATION COMPLETE");
    println!("{}", "=".repeat(80));

    Ok(())
}
