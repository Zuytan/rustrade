use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::types::{MarketEvent, Order};
use anyhow::Result;
use async_trait::async_trait;
// use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::sync::Arc;
// use std::time::Duration;
use tokio::sync::{
    RwLock,
    mpsc::{self, Receiver, Sender},
};
// use tokio::time;
use tracing::info;

pub struct MockMarketDataService {
    subscribers: Arc<RwLock<Vec<Sender<MarketEvent>>>>,
}

impl MockMarketDataService {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn publish(&self, event: MarketEvent) {
        let mut subs = self.subscribers.write().await;
        // retain only active subscribers
        let mut active_subs = Vec::new();
        for tx in subs.iter() {
            if tx.send(event.clone()).await.is_ok() {
                active_subs.push(tx.clone());
            }
        }
        *subs = active_subs;
    }
}

#[async_trait]
impl MarketDataService for MockMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        let (tx, rx) = mpsc::channel(100);

        // Add to subscribers
        self.subscribers.write().await.push(tx.clone());

        let symbols = symbols.clone();

        // OPTIONAL: Keep the random walk for "demo" mode if needed,
        // but for E2E we might want to silence it or control it.
        // For now, let's DISABLE the automatic random walk to allow full manual control in E2E.
        // If we want random walk, we should explicitly start it or have a flag.
        // Assuming the user runs in Mock mode for "demo" they might expect traffic.
        // Let's spawn a weak random walk ONLY if we are NOT in a test (hard to detect).
        // OR: Add a method `start_random_walk`.
        // Let's keep it simple: No random walk by default. The sentinel will just wait.
        // If we want "demo" behavior, we can implement a `RunMode` in config.

        // RE-ENABLING Random Walk for now because `main.rs` runs in Mock mode
        // and we barely have a way to inject data from outside in normal run.
        // BUT: For E2E tests, we want deterministic data.

        // Compromise: We spawn a task but it waits for a signal?
        // Or we just rely on `publish` for tests.
        // I will COMMENT OUT the random walk to ensure E2E is stable.
        // Users can inject data via a separate "Scenario Runner" or just by modifying this.
        /*
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(500));
            // ...
        });
        */
        info!("MockMarketDataService: Subscribed to {:?}", symbols);

        Ok(rx)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        Ok(vec![
            "AAPL".to_string(),
            "MSFT".to_string(),
            "NVDA".to_string(),
            "TSLA".to_string(),
            "GOOGL".to_string(),
        ])
    }
}

use crate::domain::portfolio::Portfolio;

pub struct MockExecutionService {
    portfolio: Arc<RwLock<Portfolio>>,
    orders: Arc<RwLock<Vec<Order>>>,
    slippage_pct: f64,         // Slippage as decimal (e.g., 0.001 = 0.1%)
    commission_per_share: f64, // Commission per share (e.g., 0.001 = $0.001/share)
}

impl MockExecutionService {
    pub fn new(portfolio: Arc<RwLock<Portfolio>>) -> Self {
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            slippage_pct: 0.0,         // Default: no slippage for existing tests
            commission_per_share: 0.0, // Default: no commission for existing tests
        }
    }

    /// Create with transaction costs (for realistic backtests)
    pub fn with_costs(
        portfolio: Arc<RwLock<Portfolio>>,
        slippage_pct: f64,
        commission_per_share: f64,
    ) -> Self {
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            slippage_pct,
            commission_per_share,
        }
    }
}

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute(&self, order: Order) -> Result<()> {
        info!("MockExecution: Placing order {}...", order.id);
        // Faster execution for tests
        // time::sleep(Duration::from_millis(200)).await;

        // Simulate execution update on the "exchange" side
        let mut port = self.portfolio.write().await;

        // Apply slippage to execution price
        let slippage_multiplier = Decimal::from_f64(match order.side {
            crate::domain::types::OrderSide::Buy => 1.0 + self.slippage_pct, // Buy higher
            crate::domain::types::OrderSide::Sell => 1.0 - self.slippage_pct, // Sell lower
        })
        .unwrap_or(Decimal::ONE);

        let execution_price = order.price * slippage_multiplier;
        let cost = execution_price * order.quantity;

        // Calculate commission
        let commission =
            Decimal::from_f64(self.commission_per_share).unwrap_or(Decimal::ZERO) * order.quantity;

        if self.slippage_pct > 0.0 || self.commission_per_share > 0.0 {
            info!(
                "MockExecution: Order {} - Slippage: {:.4}%, Commission: ${:.4}, Total Cost Impact: ${:.4}",
                order.id,
                self.slippage_pct * 100.0,
                commission,
                (execution_price - order.price).abs() * order.quantity + commission
            );
        }

        match order.side {
            crate::domain::types::OrderSide::Buy => {
                // Deduct cost + commission from cash
                port.cash -= cost + commission;
                let pos = port.positions.entry(order.symbol.clone()).or_insert(
                    crate::domain::portfolio::Position {
                        symbol: order.symbol.clone(),
                        quantity: Decimal::ZERO,
                        average_price: Decimal::ZERO,
                    },
                );

                let total_qty = pos.quantity + order.quantity;
                let total_cost = (pos.quantity * pos.average_price) + cost;
                if total_qty > Decimal::ZERO {
                    pos.average_price = total_cost / total_qty;
                }
                pos.quantity = total_qty;
            }
            crate::domain::types::OrderSide::Sell => {
                // Add proceeds minus commission to cash
                port.cash += cost - commission;
                let pos = port.positions.entry(order.symbol.clone()).or_insert(
                    crate::domain::portfolio::Position {
                        symbol: order.symbol.clone(),
                        quantity: Decimal::ZERO,
                        average_price: Decimal::ZERO,
                    },
                );
                pos.quantity -= order.quantity;
            }
        }

        // Record order
        self.orders.write().await.push(order.clone());

        info!(
            "MockExecution: Order {} placed and executed on Exchange.",
            order.id
        );
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        let port = self.portfolio.read().await;
        Ok(port.clone())
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        let orders = self.orders.read().await;
        Ok(orders.clone())
    }
}
