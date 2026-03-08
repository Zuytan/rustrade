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
    let start_time = Utc::now() - Duration::days(1);
    let base_ts = start_time.timestamp();

    // Generate 100 bars of warmup (flat)
    for i in 0..100 {
        candles.push(Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(100.0).unwrap(),
            high: Decimal::from_f64_retain(101.0).unwrap(),
            low: Decimal::from_f64_retain(99.0).unwrap(),
            close: Decimal::from_f64_retain(100.0).unwrap(),
            volume: dec!(1000),
            timestamp: base_ts + (i as i64 * 60),
        });
    }

    // Add SMC Sequence
    let smc_data = [
        (100.0, 101.0, 99.0, 99.0),   // C1: Bearish OB
        (99.0, 104.0, 99.0, 104.0),   // C2: Impulsive Bullish
        (104.0, 108.0, 103.0, 108.0), // C3: FVG Top
        (108.0, 108.0, 102.0, 102.0), // C4: Retracement
        (102.0, 105.0, 102.0, 105.0), // C5: Bullish Confirm
        (105.0, 105.0, 90.0, 90.0),   // Trigger exit
        (90.0, 90.0, 80.0, 80.0),
    ];
    let offset = 100;
    for (i, (o, h, l, c)) in smc_data.into_iter().enumerate() {
        candles.push(Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(o).unwrap(),
            high: Decimal::from_f64_retain(h).unwrap(),
            low: Decimal::from_f64_retain(l).unwrap(),
            close: Decimal::from_f64_retain(c).unwrap(),
            volume: dec!(1000),
            timestamp: base_ts + ((offset + i) as i64 * 60),
        });
    }

    // 2. Configure Simulator
    let config = AnalystConfig {
        strategy: rustrade::domain::config::StrategyConfig {
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            risk_appetite_score: Some(5),
            ..Default::default()
        },
        ..Default::default()
    };

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
