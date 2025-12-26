use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use rustrade::application::system::Application;
use rustrade::config::{Config, Mode};
use rustrade::domain::ports::ExecutionService;
use rustrade::domain::types::{MarketEvent, OrderSide};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_e2e_golden_cross_buy() -> anyhow::Result<()> {
    // Setup logging to see output with --nocapture
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();

    // 1. Setup Config (Mock Mode)
    // We can override env vars or create a Config manually
    // Since Config::from_env loads .env, we might want a manual builder or just modify the returned config.
    // For this test, manual Config construction is safer if fields are public.
    // Let's rely on default but force Mock.

    // NOTE: Config fields are public? Let's assume we can construct it or use a default.
    // To avoid breaking if Config has private fields, let's try to load from env but override relevant parts.
    let mut config = Config::from_env().unwrap_or_else(|_| Config {
        // Fallback minimal config if env missing (though .env.example exists)
        mode: Mode::Mock,
        alpaca_api_key: "".into(),
        alpaca_secret_key: "".into(),
        alpaca_base_url: "".into(),
        alpaca_ws_url: "".into(),
        symbols: vec!["BTC/USD".to_string()],
        initial_cash: Decimal::from(100_000),
        max_positions: 1,
        trade_quantity: Decimal::from(1),
        fast_sma_period: 2,
        slow_sma_period: 5,
        // sma_threshold: Decimal::from_f64(0.001).unwrap(), // Actual config uses f64? Checking file.
        sma_threshold: 0.001,
        order_cooldown_seconds: 0,
        risk_per_trade_percent: 0.01,
        max_orders_per_minute: 100,
        non_pdt_mode: false,
        dynamic_symbol_mode: false,
        dynamic_scan_interval_minutes: 60,
        strategy_mode: rustrade::config::StrategyMode::Standard,
        trend_sma_period: 50,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        trend_divergence_threshold: 0.005,
        rsi_threshold: 55.0,
        trailing_stop_atr_multiplier: 3.0,
        atr_period: 14,
        max_position_size_pct: 0.25,
        max_daily_loss_pct: 0.02,
        max_drawdown_pct: 0.10,
        consecutive_loss_limit: 3,
        slippage_pct: 0.001,
        commission_per_share: 0.001,
        trend_riding_exit_buffer_pct: 0.03,
        mean_reversion_rsi_exit: 50.0,
        mean_reversion_bb_period: 20,
    });

    config.mode = Mode::Mock;
    config.symbols = vec!["BTC/USD".to_string()];
    config.fast_sma_period = 2;
    config.slow_sma_period = 5;
    config.order_cooldown_seconds = 0; // Immediate execution

    // 2. Build Application
    let _app = Application::build(config.clone()).await?;

    // 3. Get services to interact with
    // We need to downcast or access known types.
    // Since app.market_service is Arc<dyn MarketDataService>, we need to know it's MockMarketDataService.
    // Rust doesn't support easy downcasting of Arc<dyn Trait> unless we implemented Any.
    // However, `MockMarketDataService` struct definition is available.
    // A trick: We created the app, we know it's mock.
    // BUT we stored them as trait objects.
    // We might need to unsafe cast or just instantiate services externally and pass them?
    // `Application` owns them.
    // Refactoring `Application` to allow injecting services would be best, but `build` creates them.
    // Let's see if we can trick it or if we should add a helper to `Application` for testing?
    // "downcast_ref" works if trait extends Any. `MarketDataService` likely doesn't.
    //
    // Quick fix: Re-implement `MockMarketDataService` to use a global/static or shared state that we can access from outside?
    // Better: Allow `Application` to return the concrete types if we made them generic? No.
    //
    // Simplest: Check if we can change `Application` to have public fields and just hope `Any` works or
    // modify `MarketDataService` trait to have `as_any`.

    // Let's rely on the fact that we can't easily downcast.
    // Modified Plan: Modify `MarketDataService` trait to include `as_any` or specific testing hook?
    // OR: Modify `Application::build` to take services as optional args?
    // OR: Just construct `Application` fields manually in test and skip `Application::build`?
    // `Application` fields are public!

    // We can just instantiate the services locally, then construct `Application` struct manually!
    let portfolio = std::sync::Arc::new(tokio::sync::RwLock::new(
        rustrade::domain::portfolio::Portfolio::new(),
    ));
    portfolio.write().await.cash = config.initial_cash;

    let mock_market = std::sync::Arc::new(MockMarketDataService::new());
    let mock_execution = std::sync::Arc::new(MockExecutionService::new(portfolio.clone()));

    let app = Application {
        config: config.clone(),
        market_service: mock_market.clone(),
        execution_service: mock_execution.clone(),
        portfolio: portfolio.clone(),
    };

    // 4. Run Application (BACKGROUND)
    tokio::spawn(async move {
        app.run().await.unwrap();
    });

    // Wait for agents to start
    sleep(Duration::from_millis(100)).await;

    // 5. Inject Data (Golden Cross Scenario)
    // Strategy: Fast SMA (2) crosses ABOVE Slow SMA (5).
    // We need enough data points to compute SMAs.
    // Periods: 5. So we need at least 5 points.

    // Initial State: Price Flat or downtrend.
    // P1: 100
    // P2: 100
    // P3: 100
    // P4: 100
    // P5: 100 -> Fast=100, Slow=100.

    // Upward trend to cross.
    // P6: 110 -> Fast=(100+110)/2 = 105. Slow=(100+100+100+100+110)/5 = 102.
    // CROSSOVER! 105 > 102.

    let symbol = "BTC/USD".to_string();

    // Scenario:
    // 1. Establish Baseline (100)
    // 2. Dip to trigger "Below" state (Fast < Slow)
    // 3. Rip to trigger "Above" state (Fast > Slow) -> BUY SIGNAL

    let events = vec![
        100.0, 100.0, 100.0, 100.0, 100.0, // Stable (Sma5 = 100)
        90.0,  // Dip! Fast(2)=(100+90)/2=95. Slow(5)=98. State -> BELOW.
        110.0, // Recover. Fast(2)=(90+110)/2=100. Slow(5)=(100,100,100,90,110)=100. Equal/Above?
        120.0, // Breakout! Fast(2)=115. Slow(5)=(100,100,90,110,120)=104. Cross UP! -> BUY.
    ];

    let start_time = chrono::Utc::now();
    for (i, price_f64) in events.iter().enumerate() {
        let price = Decimal::from_f64(*price_f64).unwrap();
        // Advance time by 60 sec + i * 60 sec to ensure new candles
        let timestamp = start_time + chrono::Duration::seconds(60 * (i as i64 + 1));

        mock_market
            .publish(MarketEvent::Quote {
                symbol: symbol.clone(),
                price,
                timestamp: timestamp.timestamp_millis(),
            })
            .await;
        // Give time for analysis
        sleep(Duration::from_millis(10)).await;
    }

    // Flush the aggregator by sending one more event in the future
    let flush_timestamp = start_time + chrono::Duration::seconds(60 * (events.len() as i64 + 5));
    mock_market
        .publish(MarketEvent::Quote {
            symbol: symbol.clone(),
            price: Decimal::from(120),
            timestamp: flush_timestamp.timestamp_millis(),
        })
        .await;
    sleep(Duration::from_millis(100)).await;

    sleep(Duration::from_secs(1)).await;

    // 6. Verify Execution
    // Check if an order was placed
    let orders = mock_execution.get_today_orders().await?;
    assert!(!orders.is_empty(), "Should have placed an order");

    let order = &orders[0];
    assert_eq!(order.symbol, symbol);
    assert!(matches!(order.side, OrderSide::Buy));
    // assert_eq!(order.quantity, config.trade_quantity); // Analyst uses risk-based sizing
    assert!(
        order.quantity > Decimal::ZERO,
        "Quantity should be positive"
    );

    Ok(())
}
