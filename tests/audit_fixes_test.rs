use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::risk_manager::{RiskConfig, RiskManager};
use rustrade::config::AssetClass;
use rustrade::domain::ports::ExecutionService;
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[tokio::test]
async fn test_consecutive_loss_triggers_circuit_breaker() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    // 1. Setup Portfolio with Cash and Positions
    // Start with 10k cash + 10 shares AAPL @ 100 ($1000 value)
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(10000);
    portfolio.positions.insert(
        "AAPL".to_string(),
        Position {
            symbol: "AAPL".to_string(),
            quantity: dec!(10),
            average_price: dec!(100),
        },
    );
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());

    // Set price to 100 initially
    mock_market.set_price("AAPL", dec!(100)).await;

    let (proposal_tx, proposal_rx) = mpsc::channel(100);
    let (order_tx, mut order_rx) = mpsc::channel(100);

    let risk_config = RiskConfig {
        consecutive_loss_limit: 3, // Trigger after 3 losses
        max_daily_loss_pct: 0.50,  // High limit to avoid triggering this
        max_drawdown_pct: 0.50,
        max_position_size_pct: 1.0,
        valuation_interval_seconds: 1,
        max_sector_exposure_pct: 1.0,
        sector_provider: None,
        allow_pdt_risk: false,
        pending_order_ttl_ms: None,
    };

    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 5000));

    let mut risk_manager = RiskManager::new(
        proposal_rx,
        order_tx,
        mock_exec.clone(),
        mock_market.clone(),
        state_manager,
        false, // non_pdt_mode
        AssetClass::Stock,
        risk_config,
        None,
    );

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
    mock_market.set_price("AAPL", dec!(90)).await;

    for i in 1..=3 {
        let proposal = TradeProposal {
            symbol: "AAPL".to_string(),
            side: OrderSide::Sell,
            price: dec!(90),
            quantity: dec!(1),
            order_type: OrderType::Limit,
            reason: format!("Loss Trade {}", i),
            timestamp: chrono::Utc::now().timestamp_millis(),
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
        price: dec!(150), // Profit! But system is halted
        quantity: dec!(1),
        order_type: OrderType::Limit,
        reason: "Test Halt".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    proposal_tx.send(proposal).await.unwrap();

    // The system should have triggered a halt and sent an emergency liquidation order
    let liquidation = tokio::time::timeout(std::time::Duration::from_millis(500), order_rx.recv())
        .await
        .expect("Should receive liquidation order")
        .expect("Stream closed");

    assert_eq!(liquidation.symbol, "AAPL");
    assert_eq!(liquidation.side, OrderSide::Sell);
    assert_eq!(liquidation.order_type, OrderType::Market);

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
    // No pause! We rely on real system time for TTL check via chrono::Utc::now()

    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(10000);
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());

    let (proposal_tx, proposal_rx) = mpsc::channel(100);
    let (order_tx, mut order_rx) = mpsc::channel(100);

    // Config: TTL 100ms, Check Interval 1s
    let risk_config = RiskConfig {
        pending_order_ttl_ms: Some(100),
        valuation_interval_seconds: 1,
        max_position_size_pct: 1.0,
        ..RiskConfig::default()
    };

    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 5000));

    let mut risk_manager = RiskManager::new(
        proposal_rx,
        order_tx,
        mock_exec.clone(),
        mock_market.clone(),
        state_manager.clone(),
        false,
        AssetClass::Stock,
        risk_config,
        None,
    );

    // Start RiskManager
    tokio::spawn(async move {
        risk_manager.run().await;
    });

    // 4. Send Proposal
    // We send a proposal but do NOT follow up with more proposals.
    // The RiskManager periodic loop should clean up the pending order.
    let proposal = TradeProposal {
        symbol: "MSFT".to_string(),
        side: OrderSide::Buy,
        price: dec!(300),
        quantity: dec!(10), // $3000
        order_type: OrderType::Limit,
        reason: "TTL Test".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    proposal_tx.send(proposal).await.unwrap();

    // 5. Mock Execution (The RiskManager forwards to Order Executor)
    let order = order_rx.recv().await.unwrap();
    mock_exec.execute(order).await.unwrap();

    // 6. Wait for TTL expiry (TTL = 100ms, Check Interval = 1s)
    // We wait 1.5s to ensure at least one valuation tick happens
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // 4. Verify Reservation Released (indicating pending order removed)
    // Since we can't inspect internal pending_orders map, we check reservations.
    // Initial reservation was $3000. It should be 0 after cleanup.
    let reserved_after = state_manager.get_total_reserved().await;

    assert_eq!(
        reserved_after,
        dec!(0),
        "Reservation should be released after TTL expiry"
    );
    println!("✅ Verified: Stale pending order cleaned up after TTL");
}
