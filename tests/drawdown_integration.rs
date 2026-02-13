use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rustrade::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use rustrade::application::risk_management::risk_manager::RiskManager;
use rustrade::config::AssetClass;
use rustrade::domain::ports::{ExecutionService, MarketDataService, OrderUpdate};
use rustrade::domain::risk::risk_config::RiskConfig;
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{Candle, MarketEvent, Order};
use rustrade::infrastructure::observability::Metrics;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

// --- Mocks ---

struct MockExecutionService;
#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute(&self, _order: Order) -> anyhow::Result<()> {
        Ok(())
    }
    async fn get_portfolio(&self) -> anyhow::Result<Portfolio> {
        Ok(Portfolio::new())
    }
    async fn get_today_orders(&self) -> anyhow::Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn get_open_orders(&self) -> anyhow::Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn cancel_order(&self, _id: &str, _s: &str) -> anyhow::Result<()> {
        Ok(())
    }
    async fn cancel_all_orders(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn subscribe_order_updates(
        &self,
    ) -> anyhow::Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
        let (tx, _) = tokio::sync::broadcast::channel(1);
        Ok(tx.subscribe())
    }
}

struct MockMarketData {
    prices: RwLock<HashMap<String, Decimal>>,
}

#[async_trait]
impl MarketDataService for MockMarketData {
    async fn get_prices(&self, _symbols: Vec<String>) -> anyhow::Result<HashMap<String, Decimal>> {
        Ok(self.prices.read().await.clone())
    }
    // minimal implementations for others
    async fn subscribe(&self, _s: Vec<String>) -> anyhow::Result<mpsc::Receiver<MarketEvent>> {
        let (_, rx) = mpsc::channel(1);
        Ok(rx)
    }
    async fn get_historical_bars(
        &self,
        _s: &str,
        _start: chrono::DateTime<chrono::Utc>,
        _end: chrono::DateTime<chrono::Utc>,
        _tf: &str,
    ) -> anyhow::Result<Vec<Candle>> {
        Ok(vec![])
    }
    async fn get_top_movers(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
    async fn get_tradable_assets(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_circuit_breaker_skipped_on_missing_prices() {
    // 1. Setup
    let (_proposal_tx, proposal_rx) = mpsc::channel(10);
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (order_tx, mut order_rx) = mpsc::channel(10);

    let _execution_service = Arc::new(MockExecutionService);
    let market_data = Arc::new(MockMarketData {
        prices: RwLock::new(HashMap::new()),
    });

    // Setup Portfolio with 1 position
    // Entry at $100. Qty 10. Cost Basis = $1000.
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(1000);
    portfolio.positions.insert(
        "AAPL".to_string(),
        Position {
            symbol: "AAPL".to_string(),
            quantity: dec!(10),
            average_price: dec!(100),
        },
    );
    portfolio.synchronized = true;

    // Use PortfolioStateManager to serve our mock portfolio
    // Note: In real app, PSM fetches from ExecutionService.
    // Here we need to trick PSM or mock ExecutionService to return THIS portfolio.
    // Easier: MockExecutionService returns a fixed portfolio.

    struct FixedPortfolioExecutionService {
        portfolio: RwLock<Portfolio>,
    }
    #[async_trait]
    impl ExecutionService for FixedPortfolioExecutionService {
        async fn get_portfolio(&self) -> anyhow::Result<Portfolio> {
            Ok(self.portfolio.read().await.clone())
        }
        // ... (other methods same as above)
        async fn execute(&self, _order: Order) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get_today_orders(&self) -> anyhow::Result<Vec<Order>> {
            Ok(vec![])
        }
        async fn get_open_orders(&self) -> anyhow::Result<Vec<Order>> {
            Ok(vec![])
        }
        async fn cancel_order(&self, _id: &str, _s: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn cancel_all_orders(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn subscribe_order_updates(
            &self,
        ) -> anyhow::Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
            let (tx, _) = tokio::sync::broadcast::channel(1);
            Ok(tx.subscribe())
        }
    }

    let mixed_exec_service = Arc::new(FixedPortfolioExecutionService {
        portfolio: RwLock::new(portfolio.clone()),
    });

    let psm = Arc::new(PortfolioStateManager::new(mixed_exec_service.clone(), 100));

    // Risk Config: Max Drawdown 10%
    let risk_config = RiskConfig {
        max_drawdown_pct: dec!(0.10),
        ..Default::default()
    };

    let spread_cache =
        Arc::new(rustrade::application::market_data::spread_cache::SpreadCache::new());
    let health_service = Arc::new(
        rustrade::application::monitoring::connection_health_service::ConnectionHealthService::new(
        ),
    );

    let agent_registry = std::sync::Arc::new(
        rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
            rustrade::infrastructure::observability::Metrics::new().unwrap(),
        ),
    );

    // Create RiskManager
    let mut risk_manager = RiskManager::new(
        proposal_rx,
        cmd_rx,
        order_tx,
        mixed_exec_service,
        market_data.clone(),
        psm.clone(),
        false, // non_pdt
        AssetClass::Stock,
        risk_config,
        None, // Perf monitor
        None, // Correlation
        None, // Risk State Repo
        None, // Candle Repo
        spread_cache,
        health_service,
        Metrics::default(), // Assuming default works, or mock it
        agent_registry,
    )
    .unwrap();

    // 2. Initialize Session
    // Manually set HWM to 3000 to simulate previous gains
    // Force sync
    psm.refresh().await.unwrap();
    risk_manager.initialize_session().await.unwrap();

    // We manipulate internal state to simulate a High Water Mark (HWM)
    // HWM = $1500 (implied price $150/share)
    // Current "Average" Equity = $1000 + (10 * 100) = $2000? No.
    // Cash 1000. Pos 10 * 100 = 1000. Total = 2000.
    // Let's set HWM to $3000.
    // If we use average price ($100), Equity is $2000.
    // Drawdown = (2000 - 3000) / 3000 = -33%. This SHOULD trigger panic if checked.

    {
        let state = risk_manager.get_state_mut();
        state.equity_high_water_mark = dec!(3000);
        state.session_start_equity = dec!(3000); // To avoid daily loss trigger if checks were loose
    }

    // 3. Test: Update Valuation with NO Prices
    // MarketData has empty hashmap.
    // RiskManager should skip circuit breaker.

    risk_manager.update_portfolio_valuation().await.unwrap();

    // Assert: No liquidation order sent
    // If panic occurred, we would see "LiquidationService: Placing EMERGENCY..." logs (in real run)
    // Check if order_rx received anything
    assert!(
        order_rx.try_recv().is_err(),
        "Should NOT send liquidation orders when prices are missing"
    );

    // Check if Halted
    assert!(!risk_manager.is_halted(), "Should NOT be halted");
    println!("PASSED: No fallback panic.");

    // 4. Test: Update Valuation WITH Prices (Crash)
    // Price drops to $100 (Real crash from $150 HWM basis).
    // Wait, HWM 3000 -> Equity 2000 is -33%.
    // So if we provide price $100, Equity is 2000. Drawdown -33% > Limit 10%.
    // This MUST trigger liquidation.

    {
        let mut prices = market_data.prices.write().await;
        prices.insert("AAPL".to_string(), dec!(100)); // Price $100
    }

    risk_manager.update_portfolio_valuation().await.unwrap();

    // Assert: Liquidation triggered
    // We expect 1 order (sell AAPL)
    // Wait a bit for async channel
    let order = tokio::time::timeout(Duration::from_millis(100), order_rx.recv()).await;

    assert!(order.is_ok(), "Should receive liquidation order");
    let order = order.unwrap();
    assert!(order.is_some());
    assert!(risk_manager.is_halted(), "Should be halted");
    println!("PASSED: Real crash triggers liquidation.");
}
