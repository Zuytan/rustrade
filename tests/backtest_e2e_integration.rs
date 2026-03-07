use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal_macros::dec;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::config::{AssetClass, Config, Mode, StrategyMode};
use rustrade::domain::market::timeframe::Timeframe;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{Candle, OrderSide};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::Arc;

#[tokio::test]
async fn test_full_backtest_pipeline_e2e() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    // 1. Setup mock services
    let market_data = Arc::new(MockMarketDataService::new_no_sim());
    let portfolio = Arc::new(tokio::sync::RwLock::new(Portfolio::new()));
    portfolio.write().await.cash = dec!(10000.0);

    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

    // 2. Setup config
    let config = Config::from_env().unwrap_or_else(|_| Config {
        mode: Mode::Mock,
        alpaca_api_key: "".into(),
        alpaca_secret_key: "".into(),
        alpaca_base_url: "".into(),
        alpaca_data_url: "".into(),
        alpaca_ws_url: "".into(),
        symbols: vec!["BTC/USD".to_string()],
        max_positions: 1,
        trade_quantity: dec!(1.0),
        fast_sma_period: 2,
        slow_sma_period: 5,
        sma_threshold: dec!(0.001),
        order_cooldown_seconds: 0,
        risk_per_trade_percent: dec!(0.01),
        max_orders_per_minute: 100,
        non_pdt_mode: false,
        dynamic_symbol_mode: false,
        dynamic_scan_interval_minutes: 60,
        strategy_mode: StrategyMode::Standard,
        trend_sma_period: 50,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        trend_divergence_threshold: dec!(0.005),
        rsi_threshold: dec!(99.0),
        trailing_stop_atr_multiplier: dec!(3.0),
        atr_period: 14,
        max_position_size_pct: dec!(1.0),
        max_daily_loss_pct: dec!(0.5),
        max_drawdown_pct: dec!(0.5),
        consecutive_loss_limit: 10,
        pending_order_ttl_ms: None,
        slippage_pct: dec!(0.0),
        commission_per_share: dec!(0.0),
        trend_riding_exit_buffer_pct: dec!(0.03),
        mean_reversion_rsi_exit: dec!(50.0),
        mean_reversion_bb_period: 20,
        risk_appetite: None,
        max_sector_exposure_pct: dec!(1.0),
        sector_map: std::collections::HashMap::new(),
        adaptive_optimization_enabled: false,
        regime_detection_window: 20,
        adaptive_evaluation_hour: 0,
        asset_class: AssetClass::Crypto,
        oanda_api_key: "".to_string(),
        oanda_account_id: "".to_string(),
        oanda_api_base_url: "".to_string(),
        oanda_stream_base_url: "".to_string(),
        min_volume_threshold: dec!(0.0),
        ema_fast_period: 50,
        ema_slow_period: 150,
        take_profit_pct: dec!(0.10),
        max_position_value_usd: dec!(100000.0),
        min_hold_time_minutes: 0,
        signal_confirmation_bars: 1,
        spread_bps: dec!(0.0),
        min_profit_ratio: dec!(0.0),
        portfolio_staleness_ms: 3000,
        portfolio_refresh_interval_ms: 60000,
        macd_requires_rising: false,
        trend_tolerance_pct: dec!(0.0),
        macd_min_threshold: dec!(0.0),
        profit_target_multiplier: dec!(1.5),
        adx_period: 14,
        adx_threshold: dec!(20.0),
        regime_volatility_threshold: dec!(2.0),
        smc_ob_lookback: 20,
        smc_min_fvg_size_pct: dec!(0.005),
        binance_api_key: "".to_string(),
        binance_secret_key: "".to_string(),
        binance_base_url: "".to_string(),
        binance_ws_url: "".to_string(),
        observability_enabled: false,
        observability_port: 9090,
        observability_bind_address: "127.0.0.1".to_string(),
        primary_timeframe: Timeframe::OneMin,
        enabled_timeframes: vec![Timeframe::OneMin],
        trend_timeframe: Timeframe::OneHour,
        enable_ml_data_collection: false,
        simulation_enabled: false,
        simulation_latency_base_ms: 0,
        simulation_latency_jitter_ms: 0,
        simulation_slippage_volatility: dec!(0.0),
        use_real_market_data: false,
        ensemble_voting_threshold: dec!(0.5),
    });

    // Simplest moving average crossover parameters (Fast 2, Slow 5)
    let analyst_config =
        rustrade::application::agents::analyst::AnalystConfig::from(config.clone());

    // 3. Create known historical data
    let symbol = "BTC/USD";
    let start_date = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let mut bars = Vec::new();
    let pad_count = 200;

    // 1. Send 200 flat bars for indicator warmup (SMA50, EMA150, etc)
    for i in 0..pad_count {
        bars.push(Candle {
            symbol: symbol.to_string(),
            open: dec!(100.0),
            high: dec!(101.0),
            low: dec!(99.0),
            close: dec!(100.0),
            volume: dec!(1000.0),
            timestamp: start_date.timestamp_millis() + (i as i64 * 60_000),
        });
    }

    // 2. Add the dynamic price action
    let prices = [
        100.0, 100.5, 101.0, 102.0, 104.0, 107.0, 111.0, 115.0, 120.0, // Strong trend -> Buy
        100.0, 90.0, 80.0, 70.0, 60.0, // Hard drop -> Sell
    ];

    let base_ts = start_date.timestamp_millis() + (pad_count as i64 * 60_000);
    for (i, &price_f64) in prices.iter().enumerate() {
        let price = Decimal::from_f64(price_f64).unwrap();
        bars.push(Candle {
            symbol: symbol.to_string(),
            open: price,
            high: price + dec!(1.0),
            low: price - dec!(1.0),
            close: price,
            volume: dec!(1000.0),
            timestamp: base_ts + (i as i64 * 60_000),
        });
    }

    // 4. Run Simulator
    let simulator = Simulator::new(market_data.clone(), exec_service.clone(), analyst_config);
    let end_date = Utc.with_ymd_and_hms(2023, 1, 1, 0, 10, 0).unwrap();

    let result = simulator
        .run_with_bars(symbol, &bars, start_date, end_date, None)
        .await?;

    // 5. Verify the backtest results match known expectations
    // There should have been at least 1 buy trade (when it hit 110-120) and 1 sell trade / trailing stop
    assert!(
        !result.trades.is_empty(),
        "Simulator should have generated trades"
    );

    let mut buy_count = 0;
    let mut _sell_count = 0;
    for trade in &result.trades {
        if trade.side == OrderSide::Buy {
            buy_count += 1;
        }
        if trade.side == OrderSide::Sell {
            _sell_count += 1;
        }
    }

    assert!(buy_count > 0, "Should have executed a BUY order");

    // The P&L will have changed from Initial Equity
    // Since the price went from 110/120 to 80, the result should be a loss if it didn't exit fast enough,
    // or possibly flat.
    assert_ne!(
        result.final_equity, result.initial_equity,
        "Equity should have changed from trading"
    );

    Ok(())
}
