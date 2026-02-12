use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rustrade::application::market_data::spread_cache::SpreadCache;
use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::liquidation_service::LiquidationService;
use rustrade::domain::ports::{ExecutionService, MarketDataService, OrderUpdate};
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{Candle, Order, OrderType};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

struct MockMarketService;
#[async_trait]
impl MarketDataService for MockMarketService {
    async fn subscribe(
        &self,
        _s: Vec<String>,
    ) -> Result<mpsc::Receiver<rustrade::domain::trading::types::MarketEvent>> {
        let (_, rx) = mpsc::channel(1);
        Ok(rx)
    }
    async fn get_tradable_assets(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn get_top_movers(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn get_prices(&self, _s: Vec<String>) -> Result<HashMap<String, Decimal>> {
        Ok(HashMap::new())
    }
    async fn get_historical_bars(
        &self,
        _s: &str,
        _st: chrono::DateTime<chrono::Utc>,
        _e: chrono::DateTime<chrono::Utc>,
        _t: &str,
    ) -> Result<Vec<Candle>> {
        Ok(vec![])
    }
}

struct StatefulMockExecution {
    portfolio: Arc<RwLock<Portfolio>>,
}

#[async_trait]
impl ExecutionService for StatefulMockExecution {
    async fn execute(&self, _order: Order) -> Result<()> {
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        let p = self.portfolio.read().await;
        Ok(p.clone())
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

    async fn cancel_all_orders(&self) -> Result<()> {
        Ok(())
    }

    async fn subscribe_order_updates(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
        let (_tx, rx) = tokio::sync::broadcast::channel(1);
        Ok(rx)
    }
}

#[tokio::test]
async fn test_smart_liquidation_uses_limit_when_spread_available() {
    // 1. Setup Portfolio with 1 BTC
    let mut port = Portfolio::new();
    port.positions.insert(
        "BTC/USD".to_string(),
        Position {
            symbol: "BTC/USD".to_string(),
            quantity: Decimal::from(1),
            average_price: Decimal::from(50000),
        },
    );

    let portfolio_lock = Arc::new(RwLock::new(port));
    let exec = Arc::new(StatefulMockExecution {
        portfolio: portfolio_lock,
    });

    let portfolio_manager = Arc::new(PortfolioStateManager::new(exec.clone(), 60000));
    // Force refresh to load state
    portfolio_manager.refresh().await.unwrap();

    // 2. Setup SpreadCache with Data
    let spread_cache = Arc::new(SpreadCache::new());
    // Bid: dec!(50000.0), Ask: 50100 -> Mid: 50050. Spread is available.
    spread_cache.update("BTC/USD".to_string(), 50000.0, 50100.0);

    // 3. Setup Liquidation Service
    let (order_tx, mut order_rx) = mpsc::channel(10);
    let service = LiquidationService::new(
        order_tx,
        portfolio_manager.clone(),
        Arc::new(MockMarketService),
        spread_cache.clone(),
    );

    // 4. Trigger Liquidation
    let mut current_prices = HashMap::new();
    current_prices.insert("BTC/USD".to_string(), Decimal::from(50050));

    service
        .liquidate_portfolio("Test Limit Logic", &current_prices)
        .await;

    // 5. Verify Order
    let order = tokio::time::timeout(std::time::Duration::from_millis(100), order_rx.recv())
        .await
        .expect("Should receive order")
        .expect("Channel closed");

    assert_eq!(order.symbol, "BTC/USD");
    assert_eq!(order.order_type, OrderType::Limit);
    assert_eq!(order.price, Decimal::from(50050)); // Expect mid price
}

#[tokio::test]
async fn test_smart_liquidation_falls_back_to_market_when_no_spread() {
    // 1. Setup Portfolio with 1 ETH
    let mut port = Portfolio::new();
    port.positions.insert(
        "ETH/USD".to_string(),
        Position {
            symbol: "ETH/USD".to_string(),
            quantity: Decimal::from(10),
            average_price: Decimal::from(3000),
        },
    );

    let portfolio_lock = Arc::new(RwLock::new(port));
    let exec = Arc::new(StatefulMockExecution {
        portfolio: portfolio_lock,
    });

    let portfolio_manager = Arc::new(PortfolioStateManager::new(exec.clone(), 60000));
    portfolio_manager.refresh().await.unwrap();

    // 2. Setup SpreadCache WITHOUT Data for ETH/USD
    let spread_cache = Arc::new(SpreadCache::new());
    // No update call for ETH/USD

    // 3. Setup Liquidation Service
    let (order_tx, mut order_rx) = mpsc::channel(10);
    let service = LiquidationService::new(
        order_tx,
        portfolio_manager.clone(),
        Arc::new(MockMarketService),
        spread_cache.clone(),
    );

    // 4. Trigger Liquidation
    let mut current_prices = HashMap::new();
    current_prices.insert("ETH/USD".to_string(), Decimal::from(3000));

    service
        .liquidate_portfolio("Test Market Fallback", &current_prices)
        .await;

    // 5. Verify Order
    let order = tokio::time::timeout(std::time::Duration::from_millis(100), order_rx.recv())
        .await
        .expect("Should receive order")
        .expect("Channel closed");

    assert_eq!(order.symbol, "ETH/USD");
    assert_eq!(order.order_type, OrderType::Market);
}

#[tokio::test]
async fn test_panic_mode_forces_market_order() {
    // 1. Setup Portfolio
    let mut port = Portfolio::new();
    port.positions.insert(
        "SOL/USD".to_string(),
        Position {
            symbol: "SOL/USD".to_string(),
            quantity: Decimal::from(100),
            average_price: Decimal::from(20),
        },
    );

    let portfolio_lock = Arc::new(RwLock::new(port));
    let exec = Arc::new(StatefulMockExecution {
        portfolio: portfolio_lock,
    });

    let portfolio_manager = Arc::new(PortfolioStateManager::new(exec.clone(), 60000));
    portfolio_manager.refresh().await.unwrap();

    // 2. Setup SpreadCache WITH Data (should be ignored in panic mode)
    let spread_cache = Arc::new(SpreadCache::new());
    spread_cache.update("SOL/USD".to_string(), 19.9, 20.1);

    // 3. Setup Liquidation Service
    let (order_tx, mut order_rx) = mpsc::channel(10);
    let service = LiquidationService::new(
        order_tx,
        portfolio_manager.clone(),
        Arc::new(MockMarketService),
        spread_cache.clone(),
    );

    // 4. Trigger Liquidation with NO PRICE (Panic Mode)
    let mut current_prices = HashMap::new();
    // Intentionally empty or zero price
    current_prices.insert("SOL/USD".to_string(), Decimal::ZERO);

    service
        .liquidate_portfolio("Test Panic Mode", &current_prices)
        .await;

    // 5. Verify Order
    let order = tokio::time::timeout(std::time::Duration::from_millis(100), order_rx.recv())
        .await
        .expect("Should receive order")
        .expect("Channel closed");

    assert_eq!(order.symbol, "SOL/USD");
    assert_eq!(order.order_type, OrderType::Market); // Must be Market due to panic
}
