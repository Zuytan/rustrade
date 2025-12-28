use rust_decimal::Decimal;
use rustrade::application::analyst::{Analyst, AnalystConfig};
use rustrade::application::scanner::MarketScanner;
use rustrade::application::sentinel::Sentinel;
use rustrade::application::strategies::DualSMAStrategy;
use rustrade::config::StrategyMode;
use rustrade::domain::portfolio::Portfolio;
use rustrade::domain::types::{Candle, MarketEvent, OrderSide};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};

#[tokio::test]
async fn test_repro_dynamic_empty_portfolio_buys() {
    let (market_tx, market_rx) = mpsc::channel(100);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(100);
    let (sentinel_cmd_tx, sentinel_cmd_rx) = mpsc::channel(100);

    let market_service = Arc::new(MockMarketDataService::new());
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
        sma_threshold: 0.0,
        order_cooldown_seconds: 0,
        risk_per_trade_percent: 0.1, // 10% risk
        strategy_mode: StrategyMode::Standard,
        trend_sma_period: 100,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        trend_divergence_threshold: 0.0,
        trailing_stop_atr_multiplier: 3.0,
        atr_period: 14,
        rsi_threshold: 50.0,
        trend_riding_exit_buffer_pct: 0.0,
        mean_reversion_rsi_exit: 50.0,
        mean_reversion_bb_period: 20,
        slippage_pct: 0.0,
        max_position_size_pct: 0.2,
    };
    let strategy = Arc::new(DualSMAStrategy::new(2, 3, 0.0));
    let mut analyst = Analyst::new(
        market_rx,
        proposal_tx,
        execution_service.clone(),
        strategy,
        config,
        None,
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
            volume: 100,
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
