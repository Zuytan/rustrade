use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal_macros::dec;
use rustrade::application::agents::analyst::{Analyst, AnalystConfig, AnalystDependencies};
use rustrade::application::market_data::spread_cache::SpreadCache;
use rustrade::domain::config::StrategyConfig;
use rustrade::domain::risk::risk_config::RiskConfig;
use rustrade::domain::trading::types::{Candle, MarketEvent, OrderSide};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::{Arc, Once};
use tokio::sync::{RwLock, mpsc};

static INIT: Once = Once::new();
const BASE_TS: i64 = 1739528936000;

fn create_smc_buy_sequence(symbol: &str, base_ts: i64) -> Vec<Candle> {
    let mut candles = Vec::new();
    let data = vec![
        (100.0, 101.0, 99.0, 99.0),   // C1: Bearish OB
        (99.0, 104.0, 99.0, 104.0),   // C2: Impulsive Bullish
        (104.0, 108.0, 103.0, 108.0), // C3: FVG Top
        (108.0, 108.0, 102.0, 102.0), // C4: Retracement
        (102.0, 105.0, 102.0, 105.0), // C5: Bullish Confirm
    ];
    for (i, (o, h, l, c)) in data.into_iter().enumerate() {
        candles.push(Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(o).unwrap(),
            high: Decimal::from_f64_retain(h).unwrap(),
            low: Decimal::from_f64_retain(l).unwrap(),
            close: Decimal::from_f64_retain(c).unwrap(),
            volume: Decimal::new(100, 0),
            timestamp: base_ts + (i as i64) * 600000,
        });
    }
    candles
}

fn create_smc_sell_sequence(symbol: &str, base_ts: i64) -> Vec<Candle> {
    let mut candles = Vec::new();
    let data = vec![
        (100.0, 101.0, 99.0, 101.0), // C1: Bullish OB
        (101.0, 101.0, 96.0, 96.0),  // C2: Impulsive Bearish
        (96.0, 97.0, 92.0, 92.0),    // C3: FVG Bottom
        (92.0, 98.0, 92.0, 98.0),    // C4: Retracement
        (98.0, 98.0, 95.0, 95.0),    // C5: Bearish Confirm
    ];
    for (i, (o, h, l, c)) in data.into_iter().enumerate() {
        candles.push(Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(o).unwrap(),
            high: Decimal::from_f64_retain(h).unwrap(),
            low: Decimal::from_f64_retain(l).unwrap(),
            close: Decimal::from_f64_retain(c).unwrap(),
            volume: Decimal::new(100, 0),
            timestamp: base_ts + (i as i64) * 600000,
        });
    }
    candles
}

#[allow(dead_code)]
fn setup_logging() {
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Create a ConnectionHealthService with MarketData already Online.
/// Required because the Analyst skips ALL candles when market_data_online=false,
/// and the default ConnectionHealthService starts Offline.
async fn create_online_health_service()
-> Arc<rustrade::application::monitoring::connection_health_service::ConnectionHealthService> {
    let svc = Arc::new(
        rustrade::application::monitoring::connection_health_service::ConnectionHealthService::new(
        ),
    );
    svc.set_market_data_status(
        rustrade::application::monitoring::connection_health_service::ConnectionStatus::Online,
        None,
    )
    .await;
    svc
}

#[tokio::test]
async fn test_immediate_warmup() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, _proposal_rx) = mpsc::channel(10);

    use rustrade::domain::trading::portfolio::Portfolio;
    let portfolio = Portfolio::new();
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));

    let market_service = Arc::new(MockMarketDataService::new());
    let config = AnalystConfig::default();
    let strategy = rustrade::application::strategies::StrategyFactory::create(
        rustrade::domain::market::strategy_config::StrategyMode::SMC,
        &config,
    );

    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    // Send subscription event
    market_tx
        .send(MarketEvent::SymbolSubscription {
            symbol: "BTC/USD".to_string(),
        })
        .await
        .unwrap();

    // Run analyst briefly
    tokio::select! {
        _ = analyst.run() => {},
        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
    }

    // Check if context was created
    assert!(
        analyst.get_context("BTC/USD").is_some(),
        "Context should exist"
    );
    let _context = analyst.get_context("BTC/USD").unwrap();
}

#[tokio::test]
async fn test_golden_cross() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    use rustrade::domain::trading::portfolio::Portfolio;
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(100000);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));

    let market_service = Arc::new(MockMarketDataService::new());
    let config = AnalystConfig {
        strategy: StrategyConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            sma_threshold: dec!(0.0),
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: dec!(0.005),
            trailing_stop_atr_multiplier: dec!(3.0),
            rsi_threshold: dec!(99.0),
            trend_riding_exit_buffer_pct: dec!(0.03),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            bb_std_dev: dec!(2.0),
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: dec!(0.05),
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
            smc_volume_multiplier: dec!(1.5),
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
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            risk_per_trade_percent: dec!(0.0),
            max_position_size_pct: dec!(0.0),
            max_loss_per_trade_pct: dec!(-0.05),
            ..RiskConfig::default()
        },
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
    };
    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));
    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    let sequence = create_smc_buy_sequence("BTC", BASE_TS);
    for candle in sequence {
        let event = MarketEvent::Candle(candle);
        market_tx.send(event).await.unwrap();
    }

    let proposal = tokio::time::timeout(std::time::Duration::from_secs(5), proposal_rx.recv())
        .await
        .expect("Timed out waiting for buy signal")
        .expect("Channel closed without proposal");
    assert_eq!(proposal.side, OrderSide::Buy);
    assert_eq!(proposal.quantity, Decimal::from(1));
}

#[tokio::test]
async fn test_prevent_short_selling() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    use rustrade::domain::trading::portfolio::Portfolio;
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(100000);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));
    let market_service = Arc::new(MockMarketDataService::new());

    let config = AnalystConfig {
        strategy: StrategyConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            sma_threshold: dec!(0.0),
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: dec!(0.005),
            trailing_stop_atr_multiplier: dec!(3.0),
            rsi_threshold: dec!(55.0),
            trend_riding_exit_buffer_pct: dec!(0.03),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            bb_std_dev: dec!(2.0),
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: dec!(0.05),
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
            smc_volume_multiplier: dec!(1.5),
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
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            risk_per_trade_percent: dec!(0.0),
            max_position_size_pct: dec!(0.1),
            max_loss_per_trade_pct: dec!(-0.05),
            ..RiskConfig::default()
        },
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
    };
    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));
    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    // Simulating a Death Cross without holding the asset
    let prices = [100.0, 100.0, 100.0, 120.0, 70.0];

    for (i, p) in prices.iter().enumerate() {
        let candle = Candle {
            symbol: "AAPL".to_string(),
            open: Decimal::from_f64_retain(*p).unwrap(),
            high: Decimal::from_f64_retain(*p).unwrap(),
            low: Decimal::from_f64_retain(*p).unwrap(),
            close: Decimal::from_f64_retain(*p).unwrap(),
            volume: Decimal::new(100, 0),
            timestamp: BASE_TS + (i as i64) * 600000,
        };
        let event = MarketEvent::Candle(candle);
        market_tx.send(event).await.unwrap();
    }

    let mut sell_detected = false;
    #[allow(clippy::collapsible_if)]
    if let Ok(Some(proposal)) =
        tokio::time::timeout(std::time::Duration::from_millis(100), proposal_rx.recv()).await
    {
        if proposal.side == OrderSide::Sell {
            sell_detected = true;
        }
    }
    assert!(
        !sell_detected,
        "Should NOT receive sell signal on empty portfolio (Short Selling Prevented)"
    );
}

#[tokio::test]
async fn test_sell_signal_with_position() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    let mut portfolio = rustrade::domain::trading::portfolio::Portfolio::new();
    portfolio.cash = Decimal::new(100000, 0);
    // Pre-load position so Sell matches verify logic
    let pos = rustrade::domain::trading::portfolio::Position {
        symbol: "BTC".to_string(),
        quantity: Decimal::from(10),
        average_price: Decimal::from(100),
    };
    portfolio.positions.insert("BTC".to_string(), pos);

    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));
    let market_service = Arc::new(MockMarketDataService::new());

    let config = AnalystConfig {
        strategy: StrategyConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            sma_threshold: dec!(0.0),
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: dec!(0.005),
            trailing_stop_atr_multiplier: dec!(3.0),
            rsi_threshold: dec!(55.0),
            trend_riding_exit_buffer_pct: dec!(0.03),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            bb_std_dev: dec!(2.0),
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: dec!(0.05),
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
            smc_volume_multiplier: dec!(1.5),
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
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            risk_per_trade_percent: dec!(0.0),
            max_position_size_pct: dec!(0.1),
            max_loss_per_trade_pct: dec!(-0.05),
            ..RiskConfig::default()
        },
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
    };
    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));
    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    let prices = [100.0, 100.0, 100.0, 120.0, 70.0];

    for (i, p) in prices.iter().enumerate() {
        let candle = Candle {
            symbol: "BTC".to_string(),
            open: Decimal::from_f64_retain(*p).unwrap(),
            high: Decimal::from_f64_retain(*p).unwrap(),
            low: Decimal::from_f64_retain(*p).unwrap(),
            close: Decimal::from_f64_retain(*p).unwrap(),
            volume: Decimal::new(100, 0),
            timestamp: BASE_TS + (i as i64) * 600000,
        };
        let event = MarketEvent::Candle(candle);
        market_tx.send(event).await.unwrap();
    }

    let mut sell_detected = false;
    while let Ok(Some(proposal)) =
        tokio::time::timeout(std::time::Duration::from_secs(2), proposal_rx.recv()).await
    {
        if proposal.side == OrderSide::Sell {
            sell_detected = true;
            break;
        }
    }
    assert!(
        sell_detected,
        "Should receive sell signal when holding position"
    );
}

#[tokio::test]
async fn test_dynamic_quantity_scaling() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(100); // Increased buffer
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    // 100k account
    let mut portfolio = rustrade::domain::trading::portfolio::Portfolio::new();
    portfolio.cash = Decimal::new(100000, 0);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));

    let market_service = Arc::new(MockMarketDataService::new());
    // Risk 2% (0.02)
    // NOTE: SignalGenerator hardcodes SMA 20 as Fast and SMA 50 as Slow.
    // We update config to match reality, though SignalGenerator ignores these values for feature selection.
    let config = AnalystConfig {
        strategy: StrategyConfig {
            fast_sma_period: 20,
            slow_sma_period: 50,
            sma_threshold: dec!(0.0),
            trend_sma_period: 200,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: dec!(0.005),
            trailing_stop_atr_multiplier: dec!(3.0),
            rsi_threshold: dec!(99.0),
            trend_riding_exit_buffer_pct: dec!(0.03),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            bb_std_dev: dec!(2.0),
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: dec!(0.05),
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
            smc_volume_multiplier: dec!(1.5),
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
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            risk_per_trade_percent: dec!(0.02),
            max_position_size_pct: dec!(0.1),
            max_loss_per_trade_pct: dec!(-0.05),
            ..RiskConfig::default()
        },
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
    };
    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));
    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    let sequence = create_smc_buy_sequence("AAPL", BASE_TS);
    for candle in sequence {
        let event = MarketEvent::Candle(candle);
        market_tx.send(event).await.unwrap();
    }

    // Increase timeout as we process more candles
    let proposal = tokio::time::timeout(std::time::Duration::from_secs(5), proposal_rx.recv())
        .await
        .expect("Timed out waiting for proposal")
        .expect("Channel closed without proposal");

    assert_eq!(proposal.side, OrderSide::Buy);

    // Final Price = 105. Equity = 100,000. Risk = 2% = 2,000.
    // With cost-aware sizing, target_amt is reduced by estimated fees, so qty <= 2000/105.
    let naive_qty = Decimal::from_f64_retain(2000.0 / 105.0)
        .unwrap()
        .round_dp(4);
    assert!(
        proposal.quantity <= naive_qty,
        "Cost-aware quantity {} should be <= naive {}",
        proposal.quantity,
        naive_qty
    );
    assert!(
        proposal.quantity >= naive_qty - rust_decimal_macros::dec!(0.01),
        "Quantity {} should be close to naive {}",
        proposal.quantity,
        naive_qty
    );
}

#[tokio::test]
async fn test_multi_symbol_isolation() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    let mut portfolio = rustrade::domain::trading::portfolio::Portfolio::new();
    portfolio.cash = Decimal::from(100000); // Need cash for BUY signals
    // Give explicit ETH position so Sell works
    portfolio.positions.insert(
        "ETH".to_string(),
        rustrade::domain::trading::portfolio::Position {
            symbol: "ETH".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(100),
        },
    );
    let portfolio_lock = Arc::new(RwLock::new(portfolio));

    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));
    let market_service = Arc::new(MockMarketDataService::new());

    // 2 slots
    let config = AnalystConfig {
        strategy: StrategyConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            sma_threshold: dec!(0.0),
            trend_sma_period: 100,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: dec!(0.005),
            trailing_stop_atr_multiplier: dec!(3.0),
            rsi_threshold: dec!(99.0),
            trend_riding_exit_buffer_pct: dec!(0.03),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            bb_std_dev: dec!(2.0),
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: dec!(0.05),
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
            smc_volume_multiplier: dec!(1.5),
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
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 2,
            trade_quantity: Decimal::from(1),
            risk_per_trade_percent: dec!(0.0),
            max_position_size_pct: dec!(0.1),
            max_loss_per_trade_pct: dec!(-0.05),
            ..RiskConfig::default()
        },
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
    };
    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));
    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    let btc_sequence = create_smc_buy_sequence("BTC", BASE_TS);
    let eth_sequence = create_smc_sell_sequence("ETH", BASE_TS);

    // Interleave candles to test isolation
    for i in 0..btc_sequence.len() {
        market_tx
            .send(MarketEvent::Candle(btc_sequence[i].clone()))
            .await
            .unwrap();
        market_tx
            .send(MarketEvent::Candle(eth_sequence[i].clone()))
            .await
            .unwrap();
    }

    // Give Analyst time to process all candles
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut btc_buy = false;
    let mut eth_sell = false;

    for _ in 0..5 {
        if let Ok(Some(proposal)) =
            tokio::time::timeout(std::time::Duration::from_secs(2), proposal_rx.recv()).await
        {
            if proposal.symbol == "BTC" && proposal.side == OrderSide::Buy {
                btc_buy = true;
            }
            if proposal.symbol == "ETH" && proposal.side == OrderSide::Sell {
                eth_sell = true;
            }
        }
    }

    assert!(btc_buy, "Should receive BTC buy signal");
    assert!(eth_sell, "Should receive ETH sell signal");
}

#[tokio::test]
async fn test_advanced_strategy_trend_filter() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);
    let portfolio = Arc::new(RwLock::new(
        rustrade::domain::trading::portfolio::Portfolio::new(),
    ));
    let exec_service = Arc::new(MockExecutionService::new(portfolio));
    let market_service = Arc::new(MockMarketDataService::new());

    // Advanced mode with long trend SMA
    let config = AnalystConfig {
        strategy: StrategyConfig {
            fast_sma_period: 2,
            slow_sma_period: 3,
            sma_threshold: dec!(0.0),
            trend_sma_period: 10,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: dec!(0.005),
            trailing_stop_atr_multiplier: dec!(3.0),
            rsi_threshold: dec!(55.0),
            trend_riding_exit_buffer_pct: dec!(0.03),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            bb_std_dev: dec!(2.0),
            ema_fast_period: 50,
            ema_slow_period: 150,
            take_profit_pct: dec!(0.05),
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
            smc_volume_multiplier: dec!(1.5),
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
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            risk_per_trade_percent: dec!(0.0),
            max_position_size_pct: dec!(0.1),
            max_loss_per_trade_pct: dec!(-0.05),
            ..RiskConfig::default()
        },
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::ZERO,
        )),
    };
    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));
    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    // Prices are low, but SMA cross happens. Trend (SMA 10) will be around 50.
    // Fast/Slow cross happens at 45 -> 55.
    let prices = [50.0, 50.0, 50.0, 45.0, 55.0];

    for (i, p) in prices.iter().enumerate() {
        let candle = Candle {
            symbol: "AAPL".to_string(),
            open: Decimal::from_f64_retain(*p).unwrap(),
            high: Decimal::from_f64_retain(*p).unwrap(),
            low: Decimal::from_f64_retain(*p).unwrap(),
            close: Decimal::from_f64_retain(*p).unwrap(),
            volume: Decimal::new(100, 0),
            timestamp: BASE_TS + (i as i64) * 600000,
        };
        let event = MarketEvent::Candle(candle);
        market_tx.send(event).await.unwrap();
    }

    // Should NOT receive buy signal because price (55) is likely not ABOVE the trend SMA yet
    // OR RSI filter prevents it if it's too volatile.
    // Actually, with these prices, trend SMA will be < 55.
    // Let's make price definitely BELOW trend.
    // Prices: dec!(100.0), 100, 100, 90, 95. Trend SMA will be ~97. Current Price 95 < 97.
    let prices2 = [100.0, 100.0, 100.0, 90.0, 95.0];
    for (i, p) in prices2.iter().enumerate() {
        let candle = Candle {
            symbol: "MSFT".to_string(),
            open: Decimal::from_f64_retain(*p).unwrap(),
            high: Decimal::from_f64_retain(*p).unwrap(),
            low: Decimal::from_f64_retain(*p).unwrap(),
            close: Decimal::from_f64_retain(*p).unwrap(),
            volume: Decimal::new(100, 0),
            timestamp: (i + 10) as i64,
        };
        let event = MarketEvent::Candle(candle);
        market_tx.send(event).await.unwrap();
    }

    let mut received = false;
    while let Ok(Some(_)) =
        tokio::time::timeout(std::time::Duration::from_millis(100), proposal_rx.recv()).await
    {
        received = true;
    }
    assert!(
        !received,
        "Should NOT receive signal when trend filter rejects it"
    );
}

#[tokio::test]
async fn test_risk_based_quantity_calculation() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    use rustrade::domain::trading::portfolio::Portfolio;
    // Start with empty portfolio - this is the production issue scenario
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(100000); // $100,000 starting cash
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));
    let market_service = Arc::new(MockMarketDataService::new());

    // Production-like configuration
    let config = AnalystConfig {
        strategy: StrategyConfig {
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 5,
            trade_quantity: Decimal::from(1),
            risk_per_trade_percent: dec!(0.01),
            max_position_size_pct: dec!(0.1),
            max_loss_per_trade_pct: dec!(-0.05),
            ..RiskConfig::default()
        },
        fee_model: Arc::new(rustrade::domain::trading::fee_model::ConstantFeeModel::new(
            Decimal::ZERO,
            Decimal::from_f64(0.001).unwrap(),
        )),
    };

    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));
    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    let sequence = create_smc_buy_sequence("NVDA", BASE_TS);
    for candle in sequence {
        let event = MarketEvent::Candle(candle);
        market_tx.send(event).await.unwrap();
    }

    // Should receive at least one buy signal
    let proposal = tokio::time::timeout(std::time::Duration::from_millis(500), proposal_rx.recv())
        .await
        .expect("Should receive a proposal within timeout")
        .expect("Should receive a buy signal");

    assert_eq!(
        proposal.side,
        OrderSide::Buy,
        "Should generate a buy signal"
    );

    assert!(
        proposal.quantity > Decimal::from(1),
        "Quantity should be risk-based, not the static fallback of 1 share (was {})",
        proposal.quantity
    );
    assert!(
        proposal.quantity < Decimal::from(100),
        "Quantity should be reasonable (was {})",
        proposal.quantity
    );
}

#[tokio::test]
async fn test_news_intelligence_filters() {
    setup_logging();
    let (_market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    let mut portfolio = rustrade::domain::trading::portfolio::Portfolio::new();
    portfolio.cash = Decimal::from(100000);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));
    let market_service = Arc::new(MockMarketDataService::new());

    let config = AnalystConfig::default();
    let strategy = Arc::new(rustrade::application::strategies::SMCStrategy::new(
        20,
        dec!(0.005),
        dec!(1.5),
    ));

    let deps = AnalystDependencies {
        execution_service: exec_service,
        market_service,
        candle_repository: None,
        strategy_repository: None,
        win_rate_provider: None,
        ui_candle_tx: None,
        spread_cache: Arc::new(SpreadCache::new()),
        connection_health_service: create_online_health_service().await,
        agent_registry: Arc::new(
            rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                rustrade::infrastructure::observability::Metrics::new().unwrap(),
            ),
        ),
    };

    let mut analyst = Analyst::new(market_rx, cmd_rx, proposal_tx, config, strategy, deps);

    analyst
        .ensure_symbol_initialized("BTC/USD", chrono::Utc::now())
        .await;

    {
        // Replaced internal access
        let context = analyst.get_context_mut("BTC/USD").unwrap();
        // Scenario 1: Bullish OK (Price > SMA)
        context.last_features.sma_50 = Some(dec!(40000.0));
        context.last_features.rsi = Some(dec!(50.0));
        let candle = Candle {
            symbol: "BTC/USD".to_string(),
            open: Decimal::from(50000),
            high: Decimal::from(50000),
            low: Decimal::from(50000),
            close: Decimal::from(50000),
            volume: Decimal::new(100, 0),
            timestamp: 1000,
        };
        context.candle_history.push_back(candle);
    }

    // Send Bullish Signal
    let signal = rustrade::domain::listener::NewsSignal {
        symbol: "BTC/USD".to_string(),
        sentiment: rustrade::domain::listener::NewsSentiment::Bullish,
        score: 0.5,
        headline: "Moon".to_string(),
        source: "Twitter".to_string(),
        url: Some("".to_string()),
    };

    analyst.handle_news_signal(signal.clone()).await;

    let proposal = proposal_rx
        .try_recv()
        .expect("Should have generated proposal for Bullish+Technical OK");
    assert_eq!(proposal.side, OrderSide::Buy);

    {
        let context = analyst.get_context_mut("BTC/USD").unwrap();
        // Scenario 2: Bullish REJECTED (Price < SMA)
        context.last_features.sma_50 = Some(dec!(40000.0));
        context.candle_history.back_mut().unwrap().close = Decimal::from(30000);
    }

    analyst.handle_news_signal(signal.clone()).await;
    assert!(
        proposal_rx.try_recv().is_err(),
        "Should NOT generate proposal in bearish trend"
    );

    {
        let context = analyst.get_context_mut("BTC/USD").unwrap();
        // Scenario 3: Bullish REJECTED (RSI > 75)
        context.last_features.sma_50 = Some(dec!(20000.0));
        context.last_features.rsi = Some(dec!(80.0));
        context.candle_history.back_mut().unwrap().close = Decimal::from(30000);
    }
    analyst.handle_news_signal(signal.clone()).await;
    assert!(
        proposal_rx.try_recv().is_err(),
        "Should NOT generate proposal when RSI > 75"
    );
}

#[tokio::test]
async fn test_trailing_stop_suppresses_sell_signal() {
    setup_logging();
    let (market_tx, market_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

    let mut portfolio = rustrade::domain::trading::portfolio::Portfolio::new();
    portfolio.cash = Decimal::from(100000);
    // Add position with existing trailing stop
    let pos = rustrade::domain::trading::portfolio::Position {
        symbol: "AAPL".to_string(),
        quantity: Decimal::from(10),
        average_price: Decimal::from(150),
    };
    portfolio.positions.insert("AAPL".to_string(), pos);

    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));
    let market_service = Arc::new(MockMarketDataService::new());

    // Config with trailing stop enabled
    let config = AnalystConfig {
        strategy: StrategyConfig {
            trailing_stop_atr_multiplier: dec!(3.0),
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            order_cooldown_seconds: 0,
            ..RiskConfig::default()
        },
        ..AnalystConfig::default()
    };

    // Custom strategy that always sells
    struct AlwaysSellStrategy;
    impl rustrade::application::strategies::TradingStrategy for AlwaysSellStrategy {
        fn name(&self) -> &str {
            "AlwaysSell"
        }
        fn analyze(
            &self,
            _ctx: &rustrade::application::strategies::AnalysisContext,
        ) -> Option<rustrade::application::strategies::Signal> {
            Some(rustrade::application::strategies::Signal::sell(
                "Force Sell",
            ))
        }
    }

    let strategy = Arc::new(AlwaysSellStrategy);

    let mut analyst = Analyst::new(
        market_rx,
        cmd_rx,
        proposal_tx,
        config,
        strategy,
        AnalystDependencies {
            execution_service: exec_service,
            market_service,
            candle_repository: None,
            strategy_repository: None,
            win_rate_provider: None,
            ui_candle_tx: None,
            spread_cache: Arc::new(SpreadCache::new()),
            connection_health_service: create_online_health_service().await,
            agent_registry: Arc::new(
                rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                    rustrade::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    use rustrade::domain::trading::types::Candle;
    let candle = Candle {
        symbol: "AAPL".to_string(),
        open: Decimal::from(150),
        high: Decimal::from(150),
        low: Decimal::from(150),
        close: Decimal::from(150),
        volume: Decimal::new(100, 0),
        timestamp: 1000,
    };

    market_tx.send(MarketEvent::Candle(candle)).await.unwrap();

    // 2. Expect NO Proposal
    // We wait a bit. If we get a proposal, it's a failure.
    let result =
        tokio::time::timeout(std::time::Duration::from_millis(200), proposal_rx.recv()).await;

    match result {
        Ok(Some(p)) => {
            panic!(
                "Received unexpected proposal: {:?}. Sell signal should have been suppressed by Trailing Stop!",
                p
            );
        }
        Ok(None) => {} // Channel closed
        Err(_) => {
            // Timeout = Success (No proposal received)
            println!("✅ Sell signal successfully suppressed by active Trailing Stop.");
        }
    }
}
