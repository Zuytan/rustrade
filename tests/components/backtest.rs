use rust_decimal_macros::dec;

use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;

use rustrade::application::agents::analyst::AnalystConfig;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::config::AssetClass;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;
use rustrade::infrastructure::mock::MockExecutionService;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

// Run with: cargo test --test backtest_alpaca -- --nocapture
// Note: This test requires ALPACA_API_KEY and ALPACA_API_SECRET to be set in .env or environment
#[tokio::test]
#[ignore] // Ignored by default as it requires real API keys
async fn test_backtest_strategy_on_historical_data() {
    // 1. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    // 2. Load Config / Env
    dotenvy::dotenv().ok();
    let api_key = match std::env::var("ALPACA_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("Skipping test: ALPACA_API_KEY not set");
            return;
        }
    };
    let api_secret = match std::env::var("ALPACA_SECRET_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("Skipping test: ALPACA_SECRET_KEY not set");
            return;
        }
    };
    let ws_url = std::env::var("ALPACA_WS_URL")
        .unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());
    let data_url =
        std::env::var("ALPACA_DATA_URL").unwrap_or("https://data.alpaca.markets".to_string());
    let api_base_url =
        std::env::var("ALPACA_BASE_URL").unwrap_or("https://paper-api.alpaca.markets".to_string());

    // 3. Initialize Services
    let market_service = Arc::new(AlpacaMarketDataService::new(
        api_key,
        api_secret,
        ws_url,
        data_url,
        api_base_url,
        10000.0,
        AssetClass::Stock,
        None,
    ));

    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(100000.0);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let execution_service = Arc::new(MockExecutionService::new(portfolio_lock));

    // 4. Fetch Historical Data (e.g., TSLA on a volatile day)
    let symbol = "TSLA";
    let start_date = Utc.with_ymd_and_hms(2024, 12, 20, 14, 30, 0).unwrap(); // Market Open
    let end_date = Utc.with_ymd_and_hms(2024, 12, 20, 21, 0, 0).unwrap(); // Market Close

    let config = AnalystConfig {
        fast_sma_period: 5,
        slow_sma_period: 20,
        max_positions: 1,
        trade_quantity: dec!(1.0),
        sma_threshold: dec!(0.001),
        order_cooldown_seconds: 60,
        risk_per_trade_percent: dec!(0.02),
        strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::Standard,
        trend_sma_period: 200,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        trend_divergence_threshold: dec!(0.005),
        trailing_stop_atr_multiplier: dec!(3.0),
        atr_period: 14,
        rsi_threshold: dec!(55.0),
        trend_riding_exit_buffer_pct: dec!(0.03),
        mean_reversion_rsi_exit: dec!(50.0),
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
        max_position_size_pct: dec!(0.1),
        mean_reversion_bb_period: 20,
        bb_std_dev: dec!(2.0),
        ema_fast_period: 50,
        ema_slow_period: 150,
        take_profit_pct: dec!(0.05),
        min_hold_time_minutes: 0,
        signal_confirmation_bars: 1,
        spread_bps: dec!(5.0),
        min_profit_ratio: dec!(2.0),
        macd_requires_rising: true,
        trend_tolerance_pct: dec!(0.0),
        macd_min_threshold: dec!(0.0),
        profit_target_multiplier: dec!(1.5),
        adx_period: 14,
        adx_threshold: dec!(25.0),
        smc_ob_lookback: 20,
        smc_min_fvg_size_pct: dec!(0.005),
        risk_appetite_score: None,
        breakout_lookback: 10,
        breakout_threshold_pct: dec!(0.002),
        breakout_volume_mult: dec!(1.1),
        max_loss_per_trade_pct: dec!(-0.05),
        smc_volume_multiplier: dec!(1.5),
        enable_ml_data_collection: false,
        stat_momentum_lookback: 10,
        stat_momentum_threshold: dec!(1.5),
        stat_momentum_trend_confirmation: true,
        zscore_lookback: 20,
        zscore_entry_threshold: dec!(-2.0),
        zscore_exit_threshold: dec!(0.0),
        orderflow_ofi_threshold: dec!(0.3),
        orderflow_stacked_count: 3,
        orderflow_volume_profile_lookback: 100,
    };

    let simulator = Simulator::new(market_service, execution_service, config);

    // 5. Run Simulation
    let result = simulator
        .run(symbol, start_date, end_date)
        .await
        .expect("Simulation failed");

    // 6. Assertions
    info!("Trades Executed: {}", result.trades.len());
    info!("Return: {:.2}%", result.total_return_pct);

    // assert!(!result.trades.is_empty(), "Should have executed at least one trade");
}
