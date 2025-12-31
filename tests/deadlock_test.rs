use rustrade::infrastructure::mock::MockExecutionService;
use rustrade::domain::ports::ExecutionService;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{Order, OrderSide, OrderType};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;

#[tokio::test]
async fn test_execution_service_timeouts_on_deadlock() {
    // 1. Setup Shared Portfolio
    let portfolio = Arc::new(RwLock::new(Portfolio::new()));

    // 2. Setup Service
    let service = MockExecutionService::new(portfolio.clone());

    // 3. Simulate Deadlock: Spawn a task that holds the WRITE lock for 5 seconds
    let portfolio_clone = portfolio.clone();
    tokio::spawn(async move {
        let _guard = portfolio_clone.write().await;
        // Hold lock longer than the service timeout (2s)
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    // Give the spawned task a moment to acquire the lock
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. Attempt to Use Service (Should Fail Fast)
    let start = std::time::Instant::now();
    
    // Test get_portfolio timeout
    let result = service.get_portfolio().await;
    let duration = start.elapsed();

    // 5. Verification
    assert!(result.is_err(), "Service should have returned error due to timeout");
    assert!(duration < Duration::from_secs(4), "Service should have failed fast (approx 2s), but took {:?}", duration);
    
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Deadlock detected"), "Error message should indicate deadlock/timeout, got: {}", err_msg);
}

#[tokio::test]
async fn test_execution_timeouts_on_deadlock() {
    // 1. Setup Shared Portfolio
    let portfolio = Arc::new(RwLock::new(Portfolio::new()));

    // 2. Setup Service
    let service = MockExecutionService::new(portfolio.clone());

    // 3. Simulate Deadlock
    let portfolio_clone = portfolio.clone();
    tokio::spawn(async move {
        let _guard = portfolio_clone.write().await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. Attempt Execute
    let order = Order {
        id: "deadlock_test".to_string(),
        symbol: "TEST".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(1),
        order_type: OrderType::Market,
        timestamp: 0,
    };

    let start = std::time::Instant::now();
    let result = service.execute(order).await;
    let duration = start.elapsed();

    assert!(result.is_err());
    assert!(duration < Duration::from_secs(4));
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Deadlock detected"));
}
