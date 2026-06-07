use anyhow::Result;
use async_trait::async_trait;
use rustrade::domain::ports::{ExecutionService, OrderUpdate};
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::Order;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{RwLock, broadcast};

/// A highly configurable unified MockExecutionService for integration tests.
pub struct MockExecutionService {
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub orders: Arc<RwLock<Vec<Order>>>,
    pub today_orders: Arc<RwLock<Vec<Order>>>,
    pub open_orders: Arc<RwLock<Vec<Order>>>,
    pub execute_fail_count: Arc<AtomicUsize>,
    pub execute_fail_limit: usize,
    pub cancel_all_called: Arc<AtomicBool>,
    pub cancelled_orders: Arc<RwLock<Vec<(String, String)>>>,
    pub update_sender: broadcast::Sender<OrderUpdate>,
}

#[allow(dead_code)]
impl MockExecutionService {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            portfolio: Arc::new(RwLock::new(Portfolio::new())),
            orders: Arc::new(RwLock::new(Vec::new())),
            today_orders: Arc::new(RwLock::new(Vec::new())),
            open_orders: Arc::new(RwLock::new(Vec::new())),
            execute_fail_count: Arc::new(AtomicUsize::new(0)),
            execute_fail_limit: 0,
            cancel_all_called: Arc::new(AtomicBool::new(false)),
            cancelled_orders: Arc::new(RwLock::new(Vec::new())),
            update_sender: tx,
        }
    }

    pub fn with_portfolio(portfolio: Portfolio) -> Self {
        let mut s = Self::new();
        s.portfolio = Arc::new(RwLock::new(portfolio));
        s
    }

    pub fn with_fail_limit(limit: usize) -> Self {
        let mut s = Self::new();
        s.execute_fail_limit = limit;
        s
    }
}

impl Default for MockExecutionService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute(&self, order: &Order) -> Result<()> {
        let current = self.execute_fail_count.fetch_add(1, Ordering::SeqCst);
        if current < self.execute_fail_limit {
            return Err(anyhow::anyhow!("Simulated Failure"));
        }
        let mut orders = self.orders.write().await;
        orders.push(order.clone());
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        let p = self.portfolio.read().await;
        Ok(p.clone())
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        let o = self.today_orders.read().await;
        Ok(o.clone())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let o = self.open_orders.read().await;
        Ok(o.clone())
    }

    async fn cancel_order(&self, order_id: &str, symbol: &str) -> Result<()> {
        let mut cancelled = self.cancelled_orders.write().await;
        cancelled.push((order_id.to_string(), symbol.to_string()));
        Ok(())
    }

    async fn cancel_all_orders(&self) -> Result<()> {
        self.cancel_all_called.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        Ok(self.update_sender.subscribe())
    }
}
