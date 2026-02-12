use rust_decimal_macros::dec;

use rust_decimal::Decimal;
use rustrade::application::agents::analyst::{Analyst, AnalystConfig};
use rustrade::application::agents::scanner::MarketScanner;
use rustrade::application::agents::sentinel::Sentinel;
use rustrade::application::monitoring::connection_health_service::{
    ConnectionHealthService, ConnectionStatus,
};
use rustrade::application::strategies::DualSMAStrategy;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{Candle, MarketEvent, OrderSide};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{self, Duration};

async fn create_online_health_service() -> Arc<ConnectionHealthService> {
    let svc = Arc::new(ConnectionHealthService::new());
    svc.set_market_data_status(ConnectionStatus::Online, None)
        .await;
    svc
}

#[tokio::test]
async fn test_repro_dynamic_empty_portfolio_buys() {
    let (market_tx, market_rx) = mpsc::channel(100);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(100);
    let (sentinel_cmd_tx, sentinel_cmd_rx) = mpsc::channel(100);

    let market_service = Arc::new(MockMarketDataService::new_no_sim());
    let mut initial_portfolio = Portfolio::new();
    initial_portfolio.cash = Decimal::from(10000); // Plenty of cash
    let portfolio_lock = Arc::new(RwLock::new(initial_portfolio));
    let execution_service = Arc::new(MockExecutionService::new(portfolio_lock.clone()));

    // 1. Setup Sentinel with EMPTY initial symbols
    let mut sentinel = Sentinel::new(
        market_service.clone(),
        market_tx,
        vec![],
        Some(sentinel_cmd_rx),
        create_online_health_service().await,
    );

    // 2. Setup MarketScanner (Dynamic Mode)
    let scanner = MarketScanner::new(
        market_service.clone(),
        execution_service.clone(),
        sentinel_cmd_tx,
        Duration::from_millis(100),
        true, // Enabled
    );

    // 3. Setup Analyst
    let config = AnalystConfig {
        fast_sma_period: 2,
        slow_sma_period: 3,
        max_positions: 5,
        trade_quantity: Decimal::from(1),
        sma_threshold: dec!(0.0),
        order_cooldown_seconds: 0,
        risk_per_trade_percent: dec!(0.1), // 10% risk
        strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::Standard,
        trend_sma_period: 100,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        trend_divergence_threshold: dec!(0.0),
        trailing_stop_atr_multiplier: dec!(3.0),
        atr_period: 14,
        rsi_threshold: dec!(99.0),
        trend_riding_exit_buffer_pct: dec!(0.0),
        mean_reversion_rsi_exit: dec!(50.0),
        mean_reversion_bb_period: 20,
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
        max_position_size_pct: dec!(0.2),
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
        ensemble_weights: Default::default(),
        ensemble_voting_threshold: dec!(0.5),
    };

    let strategy = Arc::new(DualSMAStrategy::new(2, 3, dec!(0.0)));
    let (_analyst_cmd_tx, analyst_cmd_rx) = mpsc::channel(10);
    let mut analyst = Analyst::new(
        market_rx,
        analyst_cmd_rx,
        proposal_tx,
        config,
        strategy,
        rustrade::application::agents::analyst::AnalystDependencies {
            execution_service: execution_service.clone(),
            market_service: market_service.clone(),
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            spread_cache: std::sync::Arc::new(
                rustrade::application::market_data::spread_cache::SpreadCache::new(),
            ),
            ui_candle_tx: None,
            connection_health_service: create_online_health_service().await,
        },
    );

    // Spawn agents
    tokio::spawn(async move { sentinel.run().await });
    tokio::spawn(async move { scanner.run().await });
    tokio::spawn(async move { analyst.run().await });

    // 4. Wait for Scanner to pick up "Top Movers" from MockMarketDataService
    // MockMarketDataService::get_top_movers returns [AAPL, MSFT, NVDA, TSLA, GOOGL]

    // 5. Inject candles for one of the moved symbols (e.g., "AAPL")
    // We send candles but wait... if Sentinel ignored the new receiver,
    // it will ONLY receive if we publish to the OLD one.
    // In our MockMarketDataService, it publishes to ALL.
    // Let's modify the MockMarketDataService in the test to ONLY publish to the LATEST subscriber
    // to simulate a service where each subscribe() is a fresh stream.

    // Wait for scanner to run at least once
    time::sleep(Duration::from_millis(200)).await;

    let prices = [100.0, 100.0, 100.0, 110.0, 120.0];
    for (i, p) in prices.iter().enumerate() {
        let event = MarketEvent::Candle(Candle {
            symbol: "AAPL".to_string(),
            open: Decimal::from_f64_retain(*p).unwrap(),
            high: Decimal::from_f64_retain(*p).unwrap(),
            low: Decimal::from_f64_retain(*p).unwrap(),
            close: Decimal::from_f64_retain(*p).unwrap(),
            volume: Decimal::new(100, 0),
            timestamp: i as i64,
        });
        market_service.publish(event).await;
        time::sleep(Duration::from_millis(50)).await;
    }

    // 6. Verify if we received a Buy proposal
    match time::timeout(Duration::from_secs(2), proposal_rx.recv()).await {
        Ok(Some(proposal)) => {
            assert_eq!(proposal.symbol, "AAPL");
            assert_eq!(proposal.side, OrderSide::Buy);
        }
        Ok(None) => panic!("Proposal channel closed"),
        Err(_) => panic!("Timeout waiting for buy proposal. It seems nothing was purchased."),
    }
}
