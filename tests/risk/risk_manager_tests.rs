//! Integration tests for RiskManager
//! Extracted from risk_manager.rs to improve file maintainability

use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::commands::RiskCommand;
use rustrade::application::risk_management::risk_manager::RiskManager;
use rustrade::application::market_data::spread_cache::SpreadCache;
use rustrade::config::AssetClass;
use rustrade::domain::ports::{ExecutionService, MarketDataService, SectorProvider};
use rustrade::domain::risk::filters::correlation_filter::CorrelationFilterConfig;
use rustrade::domain::risk::risk_config::RiskConfig;
use rustrade::domain::sentiment::{Sentiment, SentimentClassification};
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{Candle, MarketEvent, Order, OrderSide, OrderType, TradeProposal};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use rustrade::infrastructure::observability::Metrics;

use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};
use tracing::info;

// ============================================================================
// TEST HELPERS
// ============================================================================

struct ConfigurableMockMarketData {
    prices: Arc<Mutex<HashMap<String, Decimal>>>,
}

impl ConfigurableMockMarketData {
    fn new() -> Self {
        Self {
            prices: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    fn set_price(&self, symbol: &str, price: Decimal) {
        let mut prices = self.prices.lock().unwrap();
        prices.insert(symbol.to_string(), price);
    }
}

#[async_trait::async_trait]
impl MarketDataService for ConfigurableMockMarketData {
    async fn subscribe(
        &self,
        _symbols: Vec<String>,
    ) -> Result<mpsc::Receiver<MarketEvent>, anyhow::Error> {
        let (_, rx) = mpsc::channel(1);
        Ok(rx)
    }
    async fn get_tradable_assets(&self) -> Result<Vec<String>, anyhow::Error> {
        Ok(vec![])
    }
    async fn get_top_movers(&self) -> Result<Vec<String>, anyhow::Error> {
        Ok(vec![])
    }
    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<HashMap<String, Decimal>, anyhow::Error> {
        let prices = self.prices.lock().unwrap();
        let mut result = HashMap::new();
        for sym in symbols {
            if let Some(p) = prices.get(&sym) {
                result.insert(sym, *p);
            }
        }
        Ok(result)
    }
    async fn get_historical_bars(
        &self,
        _symbol: &str,
        _start: chrono::DateTime<chrono::Utc>,
        _end: chrono::DateTime<chrono::Utc>,
        _timeframe: &str,
    ) -> Result<Vec<Candle>, anyhow::Error> {
        Ok(vec![])
    }
}

struct MockSectorProvider {
    sectors: HashMap<String, String>,
}

#[async_trait::async_trait]
impl SectorProvider for MockSectorProvider {
    async fn get_sector(&self, symbol: &str) -> Result<String, anyhow::Error> {
        Ok(self
            .sectors
            .get(symbol)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string()))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[tokio::test]
async fn test_circuit_breaker_on_market_crash() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);

    // Setup Portfolio: $10,000 Cash + 100 TSLA @ $100 ($10,000 Value) = $20,000 Equity
    let mut port = Portfolio::new();
    port.cash = Decimal::from(10000);
    port.positions.insert(
        "TSLA".to_string(),
        Position {
            symbol: "TSLA".to_string(),
            quantity: Decimal::from(100),
            average_price: Decimal::from(100),
        },
    );
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

    // Setup Market: TSLA @ $100 Initially
    let market_data = Arc::new(ConfigurableMockMarketData::new());
    market_data.set_price("TSLA", Decimal::from(100));
    let market_service = market_data.clone();

    // Config: Max Daily Loss 5%
    let config = RiskConfig {
        max_daily_loss_pct: 0.10,
        valuation_interval_seconds: 1,
        correlation_config: CorrelationFilterConfig::default(),
        ..RiskConfig::default()
    };

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Stock,
        config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new()),
        Metrics::default(),
    )
    .expect("Test config should be valid");

    // Run RiskManager in background
    tokio::spawn(async move { rm.run().await });

    // Wait for initialization (should set session start equity to $20,000)
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // CRASH MARKET: TSLA -> $80 (-20%)
    // New Equity: $10k + $8k = $18k. Loss = $2k (10%). Should trigger 5% limit.
    market_data.set_price("TSLA", Decimal::from(80));

    let proposal = TradeProposal {
        symbol: "TSLA".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(80),
        quantity: Decimal::from(10),
        order_type: OrderType::Market,
        reason: "Buy the dip".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal).await.unwrap();

    // Expect Liquidation Order due to Circuit Breaker
    let liquidation_order =
        tokio::time::timeout(std::time::Duration::from_millis(200), order_rx.recv())
            .await
            .expect("Should trigger liquidation")
            .expect("Should receive liquidation order");

    assert_eq!(liquidation_order.symbol, "TSLA");
    assert_eq!(liquidation_order.side, OrderSide::Sell);
    assert_eq!(liquidation_order.order_type, OrderType::Market);

    // Ensure NO other orders (like the proposal) are processed
    assert!(
        order_rx.try_recv().is_err(),
        "Should catch only liquidation order"
    );
}

#[tokio::test]
async fn test_buy_approval() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
    let market_service = Arc::new(MockMarketDataService::new());

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Stock,
        RiskConfig::default(),
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(ConnectionHealthService::new()),
    )
    .expect("Test config should be valid");
    tokio::spawn(async move { rm.run().await });

    let proposal = TradeProposal {
        symbol: "ABC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(1),
        order_type: OrderType::Market,
        reason: "Test".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal).await.unwrap();

    let order = order_rx.recv().await.expect("Should approve");
    assert_eq!(order.symbol, "ABC");
}

#[tokio::test]
async fn test_buy_rejection_insufficient_funds() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(50); // Less than 100
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
    let market_service = Arc::new(MockMarketDataService::new());

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Stock,
        RiskConfig::default(),
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(ConnectionHealthService::new()),
    )
    .expect("Test config should be valid");
    tokio::spawn(async move { rm.run().await });

    let proposal = TradeProposal {
        symbol: "ABC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(1),
        order_type: OrderType::Market,
        reason: "Test".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal).await.unwrap();

    // Give it a moment to process (or fail to process)
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(order_rx.try_recv().is_err());
}

#[tokio::test]
async fn test_buy_rejection_insufficient_buying_power_high_equity() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    // High equity via positions
    port.positions.insert(
        "AAPL".to_string(),
        Position {
            symbol: "AAPL".to_string(),
            quantity: Decimal::from(1000),
            average_price: Decimal::from(100),
        },
    );
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

    // Mock Market Data (Need AAPL price for Equity calc)
    let market_data = Arc::new(ConfigurableMockMarketData::new());
    market_data.set_price("AAPL", Decimal::from(100)); // $100k Equity
    let market_service = market_data;

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    // Default config: 10% max position size = $10,000 (approx 10% of $101,000 equity)
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service.clone(),
        state_manager,
        false,
        AssetClass::Stock,
        RiskConfig::default(),
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(ConnectionHealthService::new()),
    )
    .expect("Test config should be valid");

    tokio::spawn(async move { rm.run().await });

    // Proposal: Buy $5,000
    // Position Size check: $5,000 < $10,100 (10% of equity). PASS.
    // Buying Power check: $5,000 > $1,000 (Available Cash). REJECT.
    let proposal = TradeProposal {
        symbol: "MSFT".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(50), // $5,000
        order_type: OrderType::Market,
        reason: "Test Buying Power".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal).await.unwrap();

    // Give it a moment to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Assert NO order was generated
    assert!(
        order_rx.try_recv().is_err(),
        "Order should be rejected due to insufficient buying power despite high equity"
    );
}

#[tokio::test]
async fn test_sell_approval() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.positions.insert(
        "ABC".to_string(),
        Position {
            symbol: "ABC".to_string(),
            quantity: Decimal::from(10), // Own 10
            average_price: Decimal::from(50),
        },
    );
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
    let market_service = Arc::new(MockMarketDataService::new());

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Stock,
        RiskConfig::default(),
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(ConnectionHealthService::new()),
    )
    .expect("Test config should be valid");
    tokio::spawn(async move { rm.run().await });

    let proposal = TradeProposal {
        symbol: "ABC".to_string(),
        side: OrderSide::Sell,
        price: Decimal::from(100),
        quantity: Decimal::from(5), // Sell 5
        order_type: OrderType::Market,
        reason: "Test".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal).await.unwrap();

    let order = order_rx.recv().await.expect("Should approve");
    assert_eq!(order.symbol, "ABC");
}

#[tokio::test]
async fn test_pdt_protection_rejection() {
    let (_proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(20000); // Trigger is_pdt_risk
    port.day_trades_count = 3; // Trigger pdt saturation
    port.positions.insert(
        "ABC".to_string(),
        Position {
            symbol: "ABC".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(50),
        },
    );
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

    // Simulate a BUY today
    exec_service
        .execute(Order {
            id: "buy1".to_string(),
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(50),
            quantity: Decimal::from(10),
            order_type: OrderType::Limit,
            status: crate::domain::trading::types::OrderStatus::Filled,
            timestamp: Utc::now().timestamp_millis(),
        })
        .await
        .unwrap();

    // New RiskManager with NON_PDT_MODE = true
    let market_service = Arc::new(MockMarketDataService::new());
    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let risk_config = RiskConfig {
        max_daily_loss_pct: 0.5, // 50% max allowed
        max_drawdown_pct: 0.5,   // 50%
        ..Default::default()
    };

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false, // non_pdt_mode = false (trigger protection)
        AssetClass::Stock,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new()),
        Metrics::default(),
    )
    .expect("Test config should be valid");

    // Initialize state (this fetches initial portfolio and prices)
    rm.initialize_session().await.unwrap();

    let proposal = TradeProposal {
        symbol: "ABC".to_string(),
        side: OrderSide::Sell,
        price: Decimal::from(60),
        quantity: Decimal::from(5),
        order_type: OrderType::Market,
        reason: "Test PDT".to_string(),
        timestamp: Utc::now().timestamp_millis(),
    };

    // Handle command directly (via Command Pattern!)
    rm.handle_command(RiskCommand::ProcessProposal(proposal))
        .await
        .unwrap();

    // Should be REJECTED (no order sent to order_rx)
    assert!(
        order_rx.try_recv().is_err(),
        "Order should have been rejected by PDT protection but was sent!"
    );
}

#[tokio::test]
async fn test_sector_exposure_limit() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);

    // Setup Portfolio: $100,000 Cash + $25,000 AAPL (Tech) = $125,000 Equity
    let mut port = Portfolio::new();
    port.cash = Decimal::from(100000);
    port.positions.insert(
        "AAPL".to_string(),
        Position {
            symbol: "AAPL".to_string(),
            quantity: Decimal::from(100),
            average_price: Decimal::from(250),
        },
    );
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

    // Setup Market
    let market_data = Arc::new(ConfigurableMockMarketData::new());
    market_data.set_price("AAPL", Decimal::from(250));
    market_data.set_price("MSFT", Decimal::from(200));
    let market_service = market_data.clone();

    // Setup Sector Provider
    let mut sectors = HashMap::new();
    sectors.insert("AAPL".to_string(), "Tech".to_string());
    sectors.insert("MSFT".to_string(), "Tech".to_string());
    let sector_provider = Arc::new(MockSectorProvider { sectors });

    let config = RiskConfig {
        max_sector_exposure_pct: 0.30,
        sector_provider: Some(sector_provider),
        ..RiskConfig::default()
    };

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Stock,
        config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new()),
        Metrics::default(),
    )
    .expect("Test config should be valid");
    tokio::spawn(async move { rm.run().await });

    // Proposal: Buy MSFT (Tech) $20,000
    // New Tech Exposure: $25,000 (AAPL) + $20,000 (MSFT) = $45,000
    // New Equity (approx): $125,000
    // Pct: 45,000 / 125,000 = 36% > 30% -> REJECT
    let proposal = TradeProposal {
        symbol: "MSFT".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(200),
        quantity: Decimal::from(100), // 100 * 200 = 20,000
        reason: "Sector Test".to_string(),
        timestamp: 0,
        order_type: OrderType::Market,
    };
    proposal_tx.send(proposal).await.unwrap();

    // Should be REJECTED
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        order_rx.try_recv().is_err(),
        "Should reject due to sector exposure"
    );
}

#[tokio::test]
async fn test_circuit_breaker_triggers_liquidation() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(10); // Buffer for liquidation orders

    // Setup Portfolio: $10,000 Cash + 10 TSLA @ $1000 ($10,000 Value) = $20,000 Equity
    let mut port = Portfolio::new();
    port.cash = Decimal::from(10000);
    port.positions.insert(
        "TSLA".to_string(),
        Position {
            symbol: "TSLA".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(1000),
        },
    );
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

    // Setup Market
    let market_data = Arc::new(ConfigurableMockMarketData::new());
    market_data.set_price("TSLA", Decimal::from(1000));
    let market_service = market_data.clone();

    // Config: Max Daily Loss 10% ($2,000)
    let config = RiskConfig {
        max_daily_loss_pct: 0.10,
        ..RiskConfig::default()
    };

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Stock,
        config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new()),
        Metrics::default(),
    )
    .expect("Test config should be valid");

    tokio::spawn(async move { rm.run().await });

    // Initialize session (Equity = $20,000)
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // CRASH SCENARIO: TSLA Drops to $700 (-30%)
    // Equity drops from $20k to $17k (-15%). This exceeds 10% limit.
    let proposal = TradeProposal {
        symbol: "TSLA".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(700),
        quantity: Decimal::from(1),
        order_type: OrderType::Market,
        reason: "Trying to catch a falling knife".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal).await.unwrap();

    // Expect liquidation order
    let liquidation_order =
        tokio::time::timeout(std::time::Duration::from_millis(200), order_rx.recv())
            .await
            .expect("Should return liquidation order")
            .expect("Should have an order");

    assert_eq!(liquidation_order.symbol, "TSLA");
    assert_eq!(liquidation_order.side, OrderSide::Sell);
    assert_eq!(liquidation_order.quantity, Decimal::from(10));
    assert_eq!(liquidation_order.order_type, OrderType::Market);

    // Verify subsequent proposals are rejected (Halted state)
    let proposal2 = TradeProposal {
        symbol: "AAPL".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(150),
        quantity: Decimal::from(1),
        order_type: OrderType::Market,
        reason: "Safe trade".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal2).await.unwrap();

    // Should receive NO orders
    let res =
        tokio::time::timeout(std::time::Duration::from_millis(100), order_rx.recv()).await;
    assert!(res.is_err(), "Should timeout because trading is halted");
}

#[tokio::test]
async fn test_crypto_daily_reset() {
    // Test that session start equity resets when day changes for Crypto
    let (_proposal_tx, proposal_rx) = mpsc::channel(1);
    let (order_tx, _order_rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(10000);
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
    let market_service = Arc::new(MockMarketDataService::new());
    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let (_, dummy_cmd_rx) = mpsc::channel(1);
    let mut rm = RiskManager::new(
        proposal_rx,
        dummy_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Crypto, // Enable Crypto mode
        RiskConfig::default(),
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(ConnectionHealthService::new()),
    )
    .expect("Test config should be valid");

    // Manually manipulate last_reset_date to yesterday
    let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
    let yesterday_ts = (Utc::now() - chrono::Duration::days(1)).timestamp();

    rm.state_manager.get_state_mut().reference_date = yesterday;
    rm.state_manager.get_state_mut().updated_at = yesterday_ts;
    rm.state_manager.get_state_mut().session_start_equity = Decimal::from(5000);
    rm.state_manager.get_state_mut().daily_drawdown_reset = false;

    rm.risk_state = rm.state_manager.get_state().clone();

    let current_equity = Decimal::from(10000);
    rm.check_daily_reset(current_equity);

    assert_eq!(
        rm.risk_state.session_start_equity, current_equity,
        "Should reset session equity to current"
    );
    assert_eq!(
        rm.risk_state.reference_date,
        Utc::now().date_naive(),
        "Should update reset date to today"
    );
}

#[tokio::test]
async fn test_sentiment_risk_adjustment() {
    let (proposal_tx, proposal_rx) = mpsc::channel(1);
    let (risk_cmd_tx, risk_cmd_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);

    // Portfolio: $10,000 Cash
    let mut port = Portfolio::new();
    port.cash = Decimal::from(10000);
    let portfolio = Arc::new(RwLock::new(port));
    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
    let market_service = Arc::new(MockMarketDataService::new());

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let risk_config = RiskConfig {
        max_position_size_pct: 0.10, // 10% normally ($1000)
        ..Default::default()
    };

    let mut rm = RiskManager::new(
        proposal_rx,
        risk_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Crypto,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new()),
        Metrics::default(),
    )
    .expect("Test config should be valid");
    tokio::spawn(async move { rm.run().await });

    // 1. Inject Sentiment: Extreme Fear (20)
    let sentiment = Sentiment {
        value: 20,
        classification: SentimentClassification::from_score(20),
        timestamp: Utc::now(),
        source: "Test".to_string(),
    };
    risk_cmd_tx
        .send(RiskCommand::UpdateSentiment(sentiment))
        .await
        .unwrap();

    // Wait for processing
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 2. Proposal: Buy $600 worth (6%)
    let proposal = TradeProposal {
        symbol: "BTC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(60000),
        quantity: Decimal::from_f64(0.01).unwrap(), // $600
        order_type: OrderType::Market,
        reason: "Test Sentiment".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal).await.unwrap();

    // 3. Verify Rejection
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        order_rx.try_recv().is_err(),
        "Should be rejected due to Sentiment adjustment"
    );

    // 4. Inject Sentiment: Greed (60)
    let sentiment_greed = Sentiment {
        value: 60,
        classification: SentimentClassification::from_score(60),
        timestamp: Utc::now(),
        source: "Test".to_string(),
    };
    risk_cmd_tx
        .send(RiskCommand::UpdateSentiment(sentiment_greed))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 5. Resend Proposal (Should Pass now)
    let proposal2 = TradeProposal {
        symbol: "BTC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(60000),
        quantity: Decimal::from_f64(0.01).unwrap(), // $600 < $1000
        order_type: OrderType::Market,
        reason: "Test Sentiment Greed".to_string(),
        timestamp: 0,
    };
    proposal_tx.send(proposal2).await.unwrap();

    // 6. Verify Acceptance
    let order = order_rx
        .recv()
        .await
        .expect("Should be approved in Greed mode");
    assert_eq!(order.symbol, "BTC");
}

#[tokio::test]
async fn test_blind_liquidation_panic_mode() {
    // 1. Setup
    let portfolio = Portfolio::new();
    let portfolio = Arc::new(RwLock::new(portfolio));

    let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
    let market_service = Arc::new(MockMarketDataService::new());

    let (_proposal_tx, proposal_rx) = mpsc::channel(1);
    let (risk_cmd_tx, risk_cmd_rx) = mpsc::channel(1);
    let (order_tx, mut order_rx) = mpsc::channel(1);

    let risk_config = RiskConfig {
        max_daily_loss_pct: 0.5,
        ..Default::default()
    };

    // Portfolio has 10 BTC
    {
        let mut p = portfolio.write().await;
        p.cash = Decimal::from(1000);
        p.positions.insert(
            "BTC".to_string(),
            Position {
                symbol: "BTC".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::from(100),
            },
        );
    }

    let state_manager = Arc::new(PortfolioStateManager::new(exec_service.clone(), 5000));

    let mut rm = RiskManager::new(
        proposal_rx,
        risk_cmd_rx,
        order_tx,
        exec_service,
        market_service,
        state_manager,
        false,
        AssetClass::Crypto,
        risk_config,
        None,
        None,
        None,
        None,
        Arc::new(SpreadCache::new()),
        Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new()),
    )
    .expect("Test config should be valid");
    tokio::spawn(async move { rm.run().await });

    // 2. Trigger Liquidation (with 0 price)
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    info!("Triggering Liquidation with NO PRICE data (Panic Mode)...");
    risk_cmd_tx
        .send(RiskCommand::CircuitBreakerTrigger)
        .await
        .unwrap();

    // 3. Expect Market Sell Order
    let order = order_rx
        .recv()
        .await
        .expect("Should receive liquidation order even without price");

    assert_eq!(order.symbol, "BTC");
    assert_eq!(order.side, OrderSide::Sell);
    assert_eq!(order.quantity, Decimal::from(10));
    assert!(
        matches!(order.order_type, OrderType::Market),
        "Must be Market order in panic mode"
    );
}
