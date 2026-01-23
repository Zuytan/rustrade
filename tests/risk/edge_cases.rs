use rust_decimal_macros::dec;
use rustrade::application::market_data::spread_cache::SpreadCache;
use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::risk_manager::RiskConfig;
use rustrade::application::risk_management::risk_manager::RiskManager;
use rustrade::config::AssetClass;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

#[tokio::test]
async fn test_pdt_protection_boundary() {
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(24999.0);
    portfolio.day_trades_count = 3; // Limit matched, should trigger protection if trying to trade under 25k
    // Initial equity = 24999.0

    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());
    let (proposal_tx, proposal_rx) = mpsc::channel(10);
    let (order_tx, mut order_rx) = mpsc::channel(10);
    let (_, dummy_cmd_rx) = mpsc::channel(1);

    let risk_config = RiskConfig::default();
    // Cache stale time 0 to force refresh
    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 0));

    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        mock_exec.clone(),
        mock_market.clone(),
        state_manager,
        false, // non_pdt_mode = false => Checks < $25k rule (PDT Enabled)
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
    )
    .expect("Test config should be valid");

    tokio::spawn(async move {
        risk_manager.run().await;
    });

    // Wait for init
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // 2. Propose a Day Trade
    let proposal = TradeProposal {
        symbol: "AAPL".to_string(),
        side: OrderSide::Buy,
        price: dec!(150.0),
        quantity: dec!(10.0),
        order_type: OrderType::Market,
        reason: "PDT Test".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    proposal_tx.send(proposal).await.unwrap();

    // 3. Expect Rejection
    let timeout = tokio::time::timeout(tokio::time::Duration::from_secs(1), order_rx.recv()).await;
    assert!(
        timeout.is_err() || timeout.unwrap().is_none(),
        "Order should be rejected due to PDT rule (< $25k)"
    );

    // 4. Update Portfolio > $25k
    {
        let mut p = portfolio.write().await;
        p.cash = dec!(25001.0);
    }

    // RiskManager needs to refresh. Since we can't force it easily without waiting for Timer or Next Proposal triggering a refresh check...
    // But PortfolioStateManager has a stale mechanism. If we initialized it with 0 stale time, it should refresh on next fetch.

    // 5. Send Proposal Again
    let proposal2 = TradeProposal {
        symbol: "AAPL".to_string(),
        side: OrderSide::Buy,
        price: dec!(150.0),
        quantity: dec!(10.0),
        order_type: OrderType::Market,
        reason: "PDT Test 2".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    proposal_tx.send(proposal2).await.unwrap();

    // 6. Expect Acceptance
    if let Ok(Some(_order)) =
        tokio::time::timeout(tokio::time::Duration::from_secs(1), order_rx.recv()).await
    {
        println!("✅ Order accepted with account > $25k");
    } else {
        panic!("Order should be accepted with account > $25k");
    }
}

#[tokio::test]
async fn test_max_daily_loss_prevents_trading() {
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(10000.0);
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());
    let (proposal_tx, proposal_rx) = mpsc::channel(10);
    let (order_tx, mut order_rx) = mpsc::channel(10);
    let (_, dummy_cmd_rx) = mpsc::channel(1);

    let risk_config = RiskConfig {
        max_daily_loss_pct: 0.05,
        valuation_interval_seconds: 1,
        ..RiskConfig::default()
    };

    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 0)); // No cache

    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        mock_exec.clone(),
        mock_market.clone(),
        state_manager,
        false,
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
    )
    .expect("Test config should be valid");

    tokio::spawn(async move {
        risk_manager.run().await;
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Simulate massive loss
    {
        let mut p = portfolio.write().await;
        p.cash = dec!(9000.0); // 10% loss
    }

    // Wait for valuation tick (interval = 1s, so wait 1.2s)
    tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

    let proposal = TradeProposal {
        symbol: "TSLA".to_string(),
        side: OrderSide::Buy,
        price: dec!(200.0),
        quantity: dec!(5.0),
        order_type: OrderType::Market,
        reason: "Loss Test".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    proposal_tx.send(proposal).await.unwrap();

    // Expect Rejection
    let timeout = tokio::time::timeout(tokio::time::Duration::from_secs(1), order_rx.recv()).await;
    assert!(
        timeout.is_err() || timeout.unwrap().is_none(),
        "Order should be rejected due to Daily Max Loss violated"
    );
}

#[tokio::test]
async fn test_circuit_breaker_on_drawdown() {
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(10000.0);
    let portfolio = Arc::new(RwLock::new(portfolio));

    let mock_exec = Arc::new(MockExecutionService::new(portfolio.clone()));
    let mock_market = Arc::new(MockMarketDataService::new());
    let (proposal_tx, proposal_rx) = mpsc::channel(10);
    let (order_tx, mut order_rx) = mpsc::channel(10);
    let (_, dummy_cmd_rx) = mpsc::channel(10);

    let risk_config = RiskConfig {
        max_drawdown_pct: 0.15,
        valuation_interval_seconds: 1,
        ..RiskConfig::default()
    };

    let state_manager = Arc::new(PortfolioStateManager::new(mock_exec.clone(), 0));

    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        mock_exec.clone(),
        mock_market.clone(),
        state_manager,
        false,
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
    )
    .expect("Test config should be valid");

    tokio::spawn(async move {
        risk_manager.run().await;
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Simulate crash > 15%
    {
        let mut p = portfolio.write().await;
        p.cash = dec!(8000.0); // 20% drawdown
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

    let proposal = TradeProposal {
        symbol: "NVDA".to_string(),
        side: OrderSide::Buy,
        price: dec!(400.0),
        quantity: dec!(1.0),
        order_type: OrderType::Market,
        reason: "Drawdown Test".to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    proposal_tx.send(proposal).await.unwrap();

    // Should reject
    let timeout = tokio::time::timeout(tokio::time::Duration::from_secs(1), order_rx.recv()).await;
    assert!(
        timeout.is_err() || timeout.unwrap().is_none(),
        "Order should be rejected due to Max Drawdown"
    );
}
