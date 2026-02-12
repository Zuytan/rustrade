use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rustrade::application::agents::analyst_config::AnalystConfig;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::domain::trading::types::Candle;
use rustrade::infrastructure::mock::MockExecutionService;
use rustrade::infrastructure::mock::MockMarketDataService;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_standard_strategy_execution_synthetic() {
    // Setup logging to see what's happening
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,rustrade=debug")
        .try_init();

    // 1. Generate Synthetic Data (uptrend followed by downtrend to trigger cross)
    let mut candles = Vec::new();
    let base_price = 100.0;
    let start_time = Utc::now() - Duration::days(1);

    // Generate 200 bars of uptrend (Price goes 100 -> 120) with some noise
    for i in 0..200 {
        let price = base_price + (i as f64 * 0.1);
        candles.push(Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(price).unwrap(),
            high: Decimal::from_f64_retain(price + 0.5).unwrap(),
            low: Decimal::from_f64_retain(price - 0.5).unwrap(),
            close: Decimal::from_f64_retain(price).unwrap(),
            volume: dec!(1000),
            timestamp: (start_time + Duration::minutes(i)).timestamp(),
        });
    }

    // Generate 200 bars of downtrend (Price goes 120 -> 100)
    for i in 0..200 {
        let price = 120.0 - (i as f64 * 0.1);
        candles.push(Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(price).unwrap(),
            high: Decimal::from_f64_retain(price + 0.5).unwrap(),
            low: Decimal::from_f64_retain(price - 0.5).unwrap(),
            close: Decimal::from_f64_retain(price).unwrap(),
            volume: dec!(1000),
            timestamp: (start_time + Duration::minutes(200 + i)).timestamp(),
        });
    }

    // 2. Configure Simulator
    let mut config = AnalystConfig::default();
    config.strategy_mode = rustrade::domain::market::strategy_config::StrategyMode::Standard;
    // Ensure thresholds are reachable
    config.sma_threshold = dec!(0.001); // 0.1%
    config.risk_appetite_score = Some(5);

    // Mock Execution Service
    let portfolio = rustrade::domain::trading::portfolio::Portfolio::new();
    let portfolio_lock = Arc::new(RwLock::new(portfolio));

    // Add cash
    {
        let mut p = portfolio_lock.write().await;
        p.cash = dec!(100000);
    }

    let execution_service = Arc::new(MockExecutionService::new(portfolio_lock));

    // Mock Market Data (not used by run_with_bars but required for constructor)
    let market_service = Arc::new(MockMarketDataService::new());

    let simulator = Simulator::new(market_service, execution_service.clone(), config);

    // 3. Run Simulation
    let end_time = start_time + Duration::minutes(400);
    // Passing spy_bars as None
    let result = simulator
        .run_with_bars("TEST", &candles, start_time, end_time, None)
        .await
        .expect("Simulation failed");

    // 4. Assertions
    println!("Trades executed: {}", result.trades.len());
    println!("Final Equity: {}", result.final_equity);
    println!("Return: {}%", result.total_return_pct);

    // We expect at least one Buy (during uptrend) and one Sell (during downtrend)
    assert!(!result.trades.is_empty(), "Should have executed trades");
}
