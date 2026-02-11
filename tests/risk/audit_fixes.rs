use rust_decimal_macros::dec;
use rustrade::application::market_data::spread_cache::SpreadCache;
use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::risk_manager::{RiskConfig, RiskManager};
use rustrade::config::AssetClass;
use rustrade::domain::ports::ExecutionService;
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use rustrade::infrastructure::observability::Metrics;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

#[tokio::test]
async fn test_consecutive_loss_triggers_circuit_breaker() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    // 1. Setup Portfolio with Cash and Positions
    // Start with 10k cash + 10 shares AAPL @ 100 ($1000 value)
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(10000.0);
    portfolio.positions.insert(
        "AAPL".to_string(),
        Position {
            symbol: "AAPL".to_string(),
            quantity: dec!(10.0),
            average_price: dec!(100.0),
        },
    );
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());

    // Set price to 100 initially
    mock_market.set_price("AAPL", dec!(100.0)).await;

    let (proposal_tx, proposal_rx) = mpsc::channel(100);
    let (order_tx, mut order_rx) = mpsc::channel(100);

    let risk_config = RiskConfig {
        consecutive_loss_limit: 3,      // Trigger after 3 losses
        max_daily_loss_pct: dec!(0.50), // High limit to avoid triggering this
        max_drawdown_pct: dec!(0.50),
        max_position_size_pct: dec!(1.0),
        valuation_interval_seconds: 1,
        max_sector_exposure_pct: dec!(1.0),
        sector_provider: None,
        allow_pdt_risk: false,
        pending_order_ttl_ms: None,
        correlation_config:
            rustrade::domain::risk::filters::correlation_filter::CorrelationFilterConfig::default(),
        volatility_config: rustrade::domain::risk::volatility_manager::VolatilityConfig::default(),
    };

    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 5000));

    let health_service = Arc::new(
        rustrade::application::monitoring::connection_health_service::ConnectionHealthService::new(
        ),
    );
    health_service
        .set_market_data_status(
            rustrade::application::monitoring::connection_health_service::ConnectionStatus::Online,
            None,
        )
        .await;

    let (_, dummy_cmd_rx) = tokio::sync::mpsc::channel(1);
    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        mock_exec.clone(),
        mock_market.clone(),
        state_manager,
        false, // non_pdt_mode
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        health_service,
        Metrics::default(),
    )
    .expect("Test config should be valid");

    // Start RiskManager
    tokio::spawn(async move {
        risk_manager.run().await;
    });

    // Allow init
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // 2. Perform 3 Losing Trades
    // To simplify, we will just SELL 1 share at a time at a loss.
    // We have 10 shares @ 100.
    // Sell 1 @ 90. (Loss $10). Consecutive Losses: 1
    // Sell 1 @ 90. (Loss $10). Consecutive Losses: 2
    // Sell 1 @ 90. (Loss $10). Consecutive Losses: 3 -> HALT

    // Set market price to 90
    mock_market.set_price("AAPL", dec!(90.0)).await;

    for _i in 1..=3 {
        let proposal = TradeProposal {
            symbol: "AAPL".to_string(),
            side: OrderSide::Sell,
            price: dec!(90.0),
            quantity: dec!(0.5),
            order_type: OrderType::Market,
            reason: format!("Loss Trade {}", _i),
            timestamp: chrono::Utc::now().timestamp_millis(),
            stop_loss: None,
            take_profit: None,
        };

        proposal_tx.send(proposal).await.unwrap();

        // Expect Order
        let order = tokio::time::timeout(std::time::Duration::from_secs(1), order_rx.recv())
            .await
            .expect("Should receive order")
            .expect("Channel closed");

        // Mock Execution (will trigger OrderUpdate with Filled status, which RiskManager processes)
        mock_exec.execute(order).await.unwrap();

        // Wait for RiskManager to process the update
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // At i=3, the circuit breaker check runs on next tick (valuation_interval=1s) or next proposal?
        // It runs in the loop.
    }

    // 3. Verify System Halted
    // Send one more proposal (even a winning one) -> Should be rejected
    let proposal = TradeProposal {
        symbol: "AAPL".to_string(),
        side: OrderSide::Sell,
        price: dec!(150.0), // Profit! But system is halted
        quantity: dec!(1.0),
        order_type: OrderType::Limit,
        reason: "Test Halt".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        stop_loss: None,
        take_profit: None,
    };

    proposal_tx.send(proposal).await.unwrap();

    // The system should have triggered a halt and sent an emergency liquidation order
    let liquidation = tokio::time::timeout(std::time::Duration::from_millis(500), order_rx.recv())
        .await
        .expect("Should receive liquidation order")
        .expect("Stream closed");

    assert_eq!(liquidation.symbol, "AAPL");
    assert_eq!(liquidation.side, OrderSide::Sell);
    assert_eq!(liquidation.order_type, OrderType::Limit); // Emergency liquidations use Limit orders with slippage tolerance

    // Verify NO other orders (the proposal itself should be rejected)
    let result = tokio::time::timeout(std::time::Duration::from_millis(100), order_rx.recv()).await;
    assert!(
        result.is_err(),
        "Should NOT receive valid proposal order after halt"
    );

    println!("✅ Verified: Consecutive losses triggered circuit breaker halt and liquidation");
}

#[tokio::test]
async fn test_pending_order_ttl_cleanup() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    // No pause! We rely on real system time for TTL check via chrono::Utc::now()

    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(10000.0);
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());

    let (proposal_tx, proposal_rx) = mpsc::channel(100);
    let (order_tx, mut order_rx) = mpsc::channel(100);

    // Config: TTL 100ms, Check Interval 1s
    let risk_config = RiskConfig {
        pending_order_ttl_ms: Some(100),
        valuation_interval_seconds: 1,
        max_position_size_pct: dec!(1.0),
        max_daily_loss_pct: dec!(0.50), // Allow 50% loss
        max_drawdown_pct: dec!(0.50),   // Allow 50% drawdown
        max_sector_exposure_pct: dec!(1.0),
        allow_pdt_risk: true, // Allow PDT risk
        ..RiskConfig::default()
    };

    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 5000));

    let health_service = Arc::new(
        rustrade::application::monitoring::connection_health_service::ConnectionHealthService::new(
        ),
    );
    health_service
        .set_market_data_status(
            rustrade::application::monitoring::connection_health_service::ConnectionStatus::Online,
            None,
        )
        .await;

    let (_, dummy_cmd_rx) = tokio::sync::mpsc::channel(1);
    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        mock_exec.clone(),
        mock_market.clone(),
        state_manager.clone(),
        false,
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        health_service,
        Metrics::default(),
    )
    .expect("Test config should be valid");

    // Start RiskManager
    tokio::spawn(async move {
        risk_manager.run().await;
    });

    // 4. Send Proposal
    // We send a proposal but do NOT follow up with more proposals.
    // The RiskManager periodic loop should clean up the pending order.
    let proposal = TradeProposal {
        symbol: "TSLA".to_string(),
        side: OrderSide::Buy,
        price: dec!(200.0),
        quantity: dec!(47.0), // Reduced to 47 * 200 = 9400 < 9500 (10000 - 5% margin)
        order_type: OrderType::Market,
        reason: "Test".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        stop_loss: None,
        take_profit: None,
    };
    proposal_tx.send(proposal).await.unwrap();

    // 5. Mock Execution (The RiskManager forwards to Order Executor)
    // We add a timeout to prevent the test from hanging indefinitely if the proposal is rejected
    let order = tokio::time::timeout(std::time::Duration::from_secs(1), order_rx.recv())
        .await
        .expect("Timed out waiting for order - Proposal likely rejected by RiskManager")
        .expect("Order channel closed unexpectedly");

    mock_exec.execute(order).await.unwrap();

    // 6. Wait for TTL expiry (TTL = 100ms, Check Interval = 1s)
    // We wait 2.5s to ensure at least one valuation tick happens
    tokio::time::sleep(tokio::time::Duration::from_millis(2500)).await;

    // 7. Verify Reservation Released (indicating pending order removed)
    // Since we can't inspect internal pending_orders map, we check reservations.
    // Initial reservation was $3000. It should be 0 after cleanup.
    let reserved_after = state_manager.get_total_reserved().await;

    assert_eq!(
        reserved_after,
        dec!(0.0),
        "Reservation should be released after TTL expiry"
    );
    println!("✅ Verified: Stale pending order cleaned up after TTL");
}
