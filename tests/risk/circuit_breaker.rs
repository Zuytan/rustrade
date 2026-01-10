use rust_decimal_macros::dec;
use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::risk_manager::{RiskConfig, RiskManager};
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{OrderSide, TradeProposal};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::test]
async fn test_circuit_breaker_triggers_on_crash() {
    // 1. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    // 2. Setup Services with Initial Portfolio
    // Start with $100k Cash + $100k Stocks (Total $200k)
    // We want to simulate a crash that drops equity below max_daily_loss (e.g. 2%)
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(100_000);
    portfolio.positions.insert(
        "TSLA".to_string(),
        Position {
            symbol: "TSLA".to_string(),
            quantity: dec!(100),       // 100 shares
            average_price: dec!(1000), // @ $1000 = $100,000 value
        },
    );
    let execution_service = Arc::new(MockExecutionService::new(Arc::new(RwLock::new(portfolio))));
    let market_service = Arc::new(MockMarketDataService::new());
    market_service.set_price("TSLA", dec!(1000)).await; // Align market price with portfolio avg price

    let state_manager = Arc::new(PortfolioStateManager::new(execution_service.clone(), 500));

    // 3. Setup Risk Manager
    let (proposal_tx, proposal_rx) = mpsc::channel(10);
    let (order_tx, mut order_rx) = mpsc::channel(10);

    let config = RiskConfig {
        pending_order_ttl_ms: None,
        max_daily_loss_pct: 0.05, // 5% limit
        max_drawdown_pct: 0.10,
        max_position_size_pct: 0.50,
        consecutive_loss_limit: 5,
        valuation_interval_seconds: 1, // Fast tick for test
        max_sector_exposure_pct: 1.0,
        sector_provider: None,
        allow_pdt_risk: false,
        correlation_config:
            rustrade::domain::risk::filters::correlation_filter::CorrelationFilterConfig::default(),
        volatility_config: rustrade::domain::risk::volatility_manager::VolatilityConfig::default(),
    };

    let (_, dummy_cmd_rx) = tokio::sync::mpsc::channel(1);
    let mut risk_manager = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        execution_service.clone(),
        market_service.clone(),
        state_manager.clone(),
        true, // Non-PDT
        rustrade::config::AssetClass::Stock,
        config,
        None,
        None,
        None,
    )
    .expect("Test config should be valid");

    // Run RiskManager in background
    tokio::spawn(async move {
        risk_manager.run().await;
    });

    // Wait for RiskManager to initialize and establish baseline equity at $1000
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // 4. Simulate Market Crash
    // Initial Equity = $200,000. 5% loss = $10,000.
    // We need TSLA to drop enough to cause > $10k loss.
    // 100 shares. Drop of $150/share = $15,000 loss (7.5%).
    // New Price = $850.

    info!("Test: Simulating Market Crash (TSLA $1000 -> $850)...");
    market_service.set_price("TSLA", dec!(850)).await;

    // Trigger a valuation update manually or wait for tick?
    // The RiskManager runs a loop with `valuation_interval`.
    // We configured it to 1s. We wait 2s.
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

    // 5. Verify Liquidation Order
    // Expect a SELL order for 100 TSLA
    let liquidation_order = order_rx.recv().await;
    assert!(
        liquidation_order.is_some(),
        "Should have received liquidation order"
    );

    let order = liquidation_order.unwrap();
    assert_eq!(order.symbol, "TSLA");
    assert_eq!(order.side, OrderSide::Sell);
    assert_eq!(order.quantity, dec!(100)); // Should sell all

    // Verify it's a Market order (based on our Change #1)
    assert!(
        matches!(
            order.order_type,
            rustrade::domain::trading::types::OrderType::Market
        ),
        "Liquidation should be Market Order"
    );

    info!("Test: Liquidation order confirmed: {:?}", order);

    // 6. Verify HALT state by sending a proposal
    info!("Test: Verifying System Halt on new proposal...");
    let proposal = TradeProposal {
        symbol: "AAPL".to_string(),
        side: OrderSide::Buy,
        price: dec!(150),
        quantity: dec!(10),
        order_type: rustrade::domain::trading::types::OrderType::Limit,
        reason: "Test".to_string(),
        timestamp: 0,
    };

    proposal_tx.send(proposal).await.unwrap();

    // We expect NO order output for this proposal, as system should be halted.
    // We wait a bit to be sure.
    let result =
        tokio::time::timeout(tokio::time::Duration::from_millis(500), order_rx.recv()).await;

    match result {
        Ok(Some(order)) => panic!(
            "TEST FAILED: Received unexpected order after HALT: {:?}",
            order
        ),
        Ok(None) => {
            panic!("TEST FAILED: RiskManager channel closed unexpectedly! Task might have crashed.")
        }
        Err(_) => {
            info!("Test: System correctly rejected new orders after Halt (Timeout confirmed).")
        }
    }
}
