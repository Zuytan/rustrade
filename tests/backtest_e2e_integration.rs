use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;

use rust_decimal_macros::dec;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::config::{AssetClass, Config, Mode};
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
    let config = Config {
        mode: Mode::Mock,
        asset_class: AssetClass::Crypto,
        broker: rustrade::config::BrokerEnvConfig::default(),
        strategy: rustrade::domain::config::StrategyConfig {
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            fast_sma_period: 2,
            slow_sma_period: 5,
            rsi_threshold: dec!(100.0),
            take_profit_pct: dec!(0.10),
            ..rustrade::domain::config::StrategyConfig::default()
        },
        risk: rustrade::domain::config::RiskConfig {
            max_positions: 1,
            trade_quantity: dec!(1.0),
            order_cooldown_seconds: 0,
            risk_per_trade_percent: dec!(0.01),
            max_position_size_pct: dec!(1.0),
            max_daily_loss_pct: dec!(0.5),
            max_drawdown_pct: dec!(0.5),
            consecutive_loss_limit: 10,
            ..rustrade::domain::config::RiskConfig::default()
        },
        platform: rustrade::config::PlatformConfig {
            symbols: vec!["BTC/USD".to_string()],
            spread_bps: dec!(0.0),
            min_profit_ratio: dec!(0.0),
            slippage_pct: dec!(0.0),
            commission_per_share: dec!(0.0),
            ..rustrade::config::PlatformConfig::default()
        },
        observability: rustrade::config::ObservabilityEnvConfig {
            enabled: false,
            ..rustrade::config::ObservabilityEnvConfig::default()
        },
        simulation: rustrade::config::SimulationEnvConfig {
            enabled: false,
            ..rustrade::config::SimulationEnvConfig::default()
        },
    };

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

    // 2. Add the dynamic price action (SMC Sequence)
    let smc_data = [
        (100.0, 101.0, 99.0, 99.0),   // C1: Bearish OB
        (99.0, 104.0, 99.0, 104.0),   // C2: Impulsive Bullish
        (104.0, 108.0, 103.0, 108.0), // C3: FVG Top
        (108.0, 108.0, 102.0, 102.0), // C4: Retracement
        (102.0, 105.0, 102.0, 105.0), // C5: Bullish Confirm
        (105.0, 105.0, 90.0, 90.0),   // Trigger exit
        (90.0, 90.0, 80.0, 80.0),
    ];
    let base_ts = start_date.timestamp_millis() + (pad_count as i64 * 60_000);
    for (i, (o, h, l, c)) in smc_data.into_iter().enumerate() {
        bars.push(Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(o).unwrap(),
            high: Decimal::from_f64_retain(h).unwrap(),
            low: Decimal::from_f64_retain(l).unwrap(),
            close: Decimal::from_f64_retain(c).unwrap(),
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
