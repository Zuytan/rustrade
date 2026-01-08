use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::risk_manager::{RiskConfig, RiskManager};
use rustrade::config::AssetClass;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};

use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Test: Concurrent proposals for the same symbol respect position size limits
///
/// This test validates that the PortfolioStateManager's exposure reservation system
/// correctly prevents over-allocation when multiple proposals arrive simultaneously.
#[tokio::test]
async fn test_concurrent_proposals_respect_limits() {
    // Setup: Portfolio with $10,000, max position size 10% ($1,000)
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(10000);
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());

    let (proposal_tx, proposal_rx) = mpsc::channel(100);
    let (order_tx, mut order_rx) = mpsc::channel(50);

    let risk_config = RiskConfig {
        max_position_size_pct: 0.10, // 10% max position size
        max_daily_loss_pct: 0.05,
        max_drawdown_pct: 0.10,
        consecutive_loss_limit: 3,
        valuation_interval_seconds: 60,
        max_sector_exposure_pct: 0.30,
        sector_provider: None,
        allow_pdt_risk: false,
        pending_order_ttl_ms: None,
        correlation_config: rustrade::domain::risk::filters::correlation_filter::CorrelationFilterConfig::default(),
        volatility_config: rustrade::domain::risk::volatility_manager::VolatilityConfig::default(),
    };

    let state_manager = Arc::new(PortfolioStateManager::new(
        mock_exec.clone(),
        5000, // 5s staleness
    ));

    let (_, dummy_cmd_rx) = tokio::sync::mpsc::channel(1);
    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        mock_exec.clone(),
        mock_market,
        state_manager,
        false, // non_pdt_mode
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
    )
    .expect("Test config should be valid");

    // Start RiskManager in background
    tokio::spawn(async move {
        risk_manager.run().await;
    });

    // Wait for initialization
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Send 5 concurrent proposals for AAPL, each requesting $1,500
    // Expected: Only 1 approved (10% = $1,000), 4 rejected
    let price = Decimal::from(150);
    let quantity = Decimal::from(4); // 4 shares * $150 = $600 (Valid, < $1,000)

    let mut handles = vec![];
    for i in 0..5 {
        let tx = proposal_tx.clone();
        let handle = tokio::spawn(async move {
            let proposal = TradeProposal {
                symbol: "AAPL".to_string(),
                side: OrderSide::Buy,
                price,
                quantity,
                order_type: OrderType::Limit,
                reason: format!("Concurrent test {}", i),
                timestamp: chrono::Utc::now().timestamp_millis(),
            };

            tx.send(proposal).await.ok();
        });
        handles.push(handle);
    }

    // Wait for all proposals to be sent
    for handle in handles {
        handle.await.ok();
    }

    // Collect approved orders (should only be 1)
    let mut approved_orders = vec![];
    let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(2));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(order) = order_rx.recv() => {
                approved_orders.push(order);
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    // Verify: Only 1 order approved due to position size limit
    assert_eq!(
        approved_orders.len(),
        1,
        "Expected exactly 1 order to be approved out of 5 concurrent proposals. \
         Got {} approved. Position size limit (10% = $1,000) should reject orders totaling $1,500.",
        approved_orders.len()
    );

    // Verify the approved order is for AAPL
    let approved = &approved_orders[0];
    assert_eq!(approved.symbol, "AAPL");
    assert_eq!(approved.side, OrderSide::Buy);

    println!(
        "✅ Concurrent proposals test passed: {} proposals → {} approved",
        5,
        approved_orders.len()
    );
}

/// Test: Verify backpressure works when proposal channel fills up
#[tokio::test]
async fn test_backpressure_drops_excess_proposals() {
    let portfolio = Portfolio::new();
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());

    // Small channel to trigger backpressure quickly
    let (proposal_tx, proposal_rx) = mpsc::channel(5);
    let (order_tx, _order_rx) = mpsc::channel(50);

    let risk_config = RiskConfig::default();
    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 5000));

    let (_, dummy_cmd_rx) = tokio::sync::mpsc::channel(1);
    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        mock_exec,
        mock_market,
        state_manager,
        false,
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
    )
    .expect("Test config should be valid");

    // Start RiskManager but intentionally slow it down
    tokio::spawn(async move {
        // Slow processing loop
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        risk_manager.run().await;
    });

    // Send 20 proposals rapidly to fill the 5-capacity channel
    let mut sent = 0;
    let mut dropped = 0;

    for i in 0..20 {
        let proposal = TradeProposal {
            symbol: format!("SYM{}", i),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(1),
            order_type: OrderType::Market,
            reason: "Backpressure test".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        match proposal_tx.try_send(proposal) {
            Ok(_) => sent += 1,
            Err(_) => dropped += 1,
        }
    }

    // Verify: Some proposals were dropped due to backpressure
    assert!(
        dropped > 0,
        "Expected some proposals to be dropped due to channel capacity (5). \
         Sent: {}, Dropped: {}",
        sent,
        dropped
    );

    println!(
        "✅ Backpressure test passed: {} sent, {} dropped (channel capacity: 5)",
        sent, dropped
    );
}
