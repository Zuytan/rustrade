//! Integration tests for RiskManager extracted services
//!
//! Tests the composition and interaction of:
//! - SessionManager
//! - PortfolioValuationService
//! - LiquidationService
//! - RiskManager orchestration

use anyhow::Result;
use rust_decimal::Decimal;
use rustrade::application::market_data::spread_cache::SpreadCache;
use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::liquidation_service::LiquidationService;
use rustrade::application::risk_management::portfolio_valuation_service::PortfolioValuationService;
use rustrade::application::risk_management::session_manager::SessionManager;
use rustrade::config::AssetClass;
use rustrade::domain::ports::{ExecutionService, MarketDataService};
use rustrade::domain::repositories::RiskStateRepository;
use rustrade::domain::risk::state::RiskState;
use rustrade::domain::risk::volatility_manager::{VolatilityConfig, VolatilityManager};
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{Candle, MarketEvent, Order};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

// ===== Mock Implementations =====

struct MockRiskStateRepo {
    state: Arc<RwLock<Option<RiskState>>>,
}

#[async_trait::async_trait]
impl RiskStateRepository for MockRiskStateRepo {
    async fn save(&self, state: &RiskState) -> Result<()> {
        *self.state.write().await = Some(state.clone());
        Ok(())
    }

    async fn load(&self, _id: &str) -> Result<Option<RiskState>> {
        Ok(self.state.read().await.clone())
    }
}

struct MockMarketData {
    prices: HashMap<String, Decimal>,
    candles: Vec<Candle>,
}

#[async_trait::async_trait]
impl MarketDataService for MockMarketData {
    async fn subscribe(&self, _symbols: Vec<String>) -> Result<mpsc::Receiver<MarketEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn get_prices(&self, _symbols: Vec<String>) -> Result<HashMap<String, Decimal>> {
        Ok(self.prices.clone())
    }

    async fn get_historical_bars(
        &self,
        _symbol: &str,
        _start: chrono::DateTime<chrono::Utc>,
        _end: chrono::DateTime<chrono::Utc>,
        _timeframe: &str,
    ) -> Result<Vec<Candle>> {
        Ok(self.candles.clone())
    }
}

struct MockExecution {
    orders: Arc<RwLock<Vec<Order>>>,
}

#[async_trait::async_trait]
impl ExecutionService for MockExecution {
    async fn execute(&self, order: Order) -> Result<()> {
        self.orders.write().await.push(order.clone());
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<rustrade::domain::trading::portfolio::Portfolio> {
        Ok(Portfolio::new())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        Ok(vec![])
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        Ok(vec![])
    }

    async fn cancel_order(&self, _order_id: &str) -> Result<()> {
        Ok(())
    }

    async fn subscribe_order_updates(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<rustrade::domain::ports::OrderUpdate>> {
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        drop(tx); // Drop sender to close channel
        Ok(rx)
    }
}

// ===== Integration Tests =====

#[tokio::test]
async fn test_session_manager_integration() {
    // Setup
    let repo = Arc::new(MockRiskStateRepo {
        state: Arc::new(RwLock::new(None)),
    });

    let mut prices = HashMap::new();
    prices.insert("AAPL".to_string(), Decimal::from(150));

    let market = Arc::new(MockMarketData {
        prices: prices.clone(),
        candles: vec![],
    });

    let session_manager = SessionManager::new(Some(repo.clone()), market);

    // Create portfolio
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(10000);
    portfolio.positions.insert(
        "AAPL".to_string(),
        Position {
            symbol: "AAPL".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(140),
        },
    );

    // Test: Initialize session
    let mut current_prices = HashMap::new();
    let state = session_manager
        .initialize_session(&portfolio, &mut current_prices)
        .await
        .expect("Session initialization should succeed");

    // Verify
    let expected_equity = Decimal::from(11500); // 10000 cash + (10 * 150)
    assert_eq!(state.session_start_equity, expected_equity);
    assert_eq!(state.daily_start_equity, expected_equity);
    assert_eq!(state.equity_high_water_mark, expected_equity);

    // Verify state was persisted
    let loaded_state = repo.load("global").await.unwrap();
    assert!(loaded_state.is_some());
    assert_eq!(loaded_state.unwrap().session_start_equity, expected_equity);
}

#[tokio::test]
async fn test_portfolio_valuation_service_integration() {
    // Setup
    let mut prices = HashMap::new();
    prices.insert("AAPL".to_string(), Decimal::from(160));
    prices.insert("GOOGL".to_string(), Decimal::from(140));

    let market = Arc::new(MockMarketData {
        prices: prices.clone(),
        candles: vec![],
    });

    let exec = Arc::new(MockExecution {
        orders: Arc::new(RwLock::new(vec![])),
    });

    let portfolio_manager = Arc::new(PortfolioStateManager::new(exec, 60000));

    let volatility_manager = Arc::new(RwLock::new(VolatilityManager::new(
        VolatilityConfig::default(),
    )));

    let valuation_service = PortfolioValuationService::new(
        market,
        portfolio_manager,
        volatility_manager,
        AssetClass::Stock,
    );

    // Test: Update portfolio valuation
    let mut current_prices = HashMap::new();
    let result = valuation_service
        .update_portfolio_valuation(&mut current_prices)
        .await;

    // Verify
    assert!(result.is_ok());
    let (_portfolio, equity) = result.unwrap();

    // Empty portfolio should have zero equity
    assert_eq!(equity, Decimal::ZERO);

    // Prices should be cached
    assert_eq!(current_prices.len(), 0); // No positions, so no prices fetched
}

#[tokio::test]
async fn test_liquidation_service_integration() {
    // Setup
    let (order_tx, mut order_rx) = mpsc::channel(10);

    let exec = Arc::new(MockExecution {
        orders: Arc::new(RwLock::new(vec![])),
    });

    // Create portfolio with positions
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(10000);
    portfolio.positions.insert(
        "AAPL".to_string(),
        Position {
            symbol: "AAPL".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(150),
        },
    );
    portfolio.positions.insert(
        "GOOGL".to_string(),
        Position {
            symbol: "GOOGL".to_string(),
            quantity: Decimal::from(5),
            average_price: Decimal::from(140),
        },
    );

    // Update portfolio manager with test portfolio
    let portfolio_manager = Arc::new(PortfolioStateManager::new(exec, 60000));
    let spread_cache = Arc::new(SpreadCache::new()); // NEW

    let liquidation_service =
        LiquidationService::new(order_tx, portfolio_manager.clone(), spread_cache.clone());

    // Manually set portfolio state for testing
    // (In real scenario, this would be updated via refresh)

    let mut prices = HashMap::new();
    prices.insert("AAPL".to_string(), Decimal::from(160));
    prices.insert("GOOGL".to_string(), Decimal::from(145));

    // Test: Liquidate portfolio
    liquidation_service
        .liquidate_portfolio("Test circuit breaker", &prices)
        .await;

    // Verify: Should receive liquidation orders
    // Note: This test is limited because we can't easily inject portfolio state
    // In a real integration test, we'd use a more sophisticated mock

    // Try to receive orders (may timeout if portfolio is empty in mock)
    tokio::time::timeout(std::time::Duration::from_millis(100), order_rx.recv())
        .await
        .ok();

    // Test passes if no panic occurs
}

#[tokio::test]
async fn test_service_composition_full_workflow() {
    // Setup all services
    let repo = Arc::new(MockRiskStateRepo {
        state: Arc::new(RwLock::new(None)),
    });

    let mut prices = HashMap::new();
    prices.insert("AAPL".to_string(), Decimal::from(150));

    let market = Arc::new(MockMarketData {
        prices: prices.clone(),
        candles: vec![],
    });

    let exec = Arc::new(MockExecution {
        orders: Arc::new(RwLock::new(vec![])),
    });

    let portfolio_manager = Arc::new(PortfolioStateManager::new(exec.clone(), 60000));
    let (order_tx, _order_rx) = mpsc::channel(10);
    let spread_cache = Arc::new(SpreadCache::new()); // NEW

    // Initialize services
    let session_manager = SessionManager::new(Some(repo.clone()), market.clone());

    let volatility_manager = Arc::new(RwLock::new(VolatilityManager::new(
        VolatilityConfig::default(),
    )));

    let valuation_service = PortfolioValuationService::new(
        market.clone(),
        portfolio_manager.clone(),
        volatility_manager,
        AssetClass::Stock,
    );

    let liquidation_service =
        LiquidationService::new(order_tx, portfolio_manager.clone(), spread_cache.clone());

    // Test workflow: Initialize -> Valuate -> (Conditional) Liquidate

    // Step 1: Initialize session
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(10000);

    let mut current_prices = HashMap::new();
    let state = session_manager
        .initialize_session(&portfolio, &mut current_prices)
        .await
        .expect("Session init should succeed");

    assert_eq!(state.session_start_equity, Decimal::from(10000));

    // Step 2: Update valuation
    let result = valuation_service
        .update_portfolio_valuation(&mut current_prices)
        .await;

    assert!(result.is_ok());

    // Step 3: Simulate circuit breaker trigger
    liquidation_service
        .liquidate_portfolio("Integration test", &current_prices)
        .await;

    // Verify: Full workflow completed without errors
    // Verify: Full service composition workflow succeeded
}

#[tokio::test]
async fn test_session_manager_daily_reset() {
    // Setup
    let yesterday = chrono::Utc::now().date_naive() - chrono::Duration::days(1);

    let existing_state = RiskState {
        id: "global".to_string(),
        session_start_equity: Decimal::from(10000),
        daily_start_equity: Decimal::from(10000),
        equity_high_water_mark: Decimal::from(12000),
        consecutive_losses: 2,
        reference_date: yesterday,
        updated_at: chrono::Utc::now().timestamp(),
        daily_drawdown_reset: false,
    };

    let repo = Arc::new(MockRiskStateRepo {
        state: Arc::new(RwLock::new(Some(existing_state))),
    });

    let market = Arc::new(MockMarketData {
        prices: HashMap::new(),
        candles: vec![],
    });

    let session_manager = SessionManager::new(Some(repo), market);

    // Test: Initialize session with old state
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(10000);
    let mut current_prices = HashMap::new();

    let state = session_manager
        .initialize_session(&portfolio, &mut current_prices)
        .await
        .expect("Should initialize");

    // Verify: HWM and consecutive losses restored, but daily equity reset
    assert_eq!(state.equity_high_water_mark, Decimal::from(12000));
    assert_eq!(state.consecutive_losses, 2);
    assert_eq!(state.reference_date, chrono::Utc::now().date_naive());
}
