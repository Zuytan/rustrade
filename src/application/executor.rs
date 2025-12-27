use crate::domain::portfolio::{Portfolio, Position};
use crate::domain::ports::ExecutionService;
use crate::domain::types::{Order, OrderSide};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;
use tracing::{error, info};

pub struct Executor {
    execution_service: Arc<dyn ExecutionService>,
    order_rx: Receiver<Order>,
    portfolio: Arc<RwLock<Portfolio>>,
    repository: Option<Arc<crate::infrastructure::persistence::repositories::OrderRepository>>,
}

impl Executor {
    pub fn new(
        execution_service: Arc<dyn ExecutionService>,
        order_rx: Receiver<Order>,
        portfolio: Arc<RwLock<Portfolio>>,
        repository: Option<Arc<crate::infrastructure::persistence::repositories::OrderRepository>>,
    ) -> Self {
        Self {
            execution_service,
            order_rx,
            portfolio,
            repository,
        }
    }

    pub async fn run(&mut self) {
        info!("Executor started.");

        while let Some(order) = self.order_rx.recv().await {
            info!(
                "Executor: Received Order {}. Executing via Service...",
                order.id
            );

            // 1. Execute External
            match self.execution_service.execute(order.clone()).await {
                Ok(_) => {
                    // 2. Update Internal State (Optimistic)
                    let mut portfolio = self.portfolio.write().await;
                    let cost = order.price * order.quantity;

                    match order.side {
                        OrderSide::Buy => {
                            portfolio.cash -= cost;
                            let position = portfolio
                                .positions
                                .entry(order.symbol.clone())
                                .or_insert(Position {
                                    symbol: order.symbol.clone(),
                                    quantity: rust_decimal::Decimal::ZERO,
                                    average_price: rust_decimal::Decimal::ZERO,
                                });

                            let total_val = (position.quantity * position.average_price) + cost;
                            let new_qty = position.quantity + order.quantity;
                            if !new_qty.is_zero() {
                                position.average_price = total_val / new_qty;
                            }
                            position.quantity = new_qty;
                        }
                        OrderSide::Sell => {
                            portfolio.cash += cost;
                            if let Some(position) = portfolio.positions.get_mut(&order.symbol) {
                                position.quantity -= order.quantity;
                            }
                        }
                    }

                    info!(
                        "Executor: Order {} processed internally. New Cash: {}",
                        order.id, portfolio.cash
                    );
                }
                Err(e) => {
                    error!("Executor: Execution failed for {}: {}", order.id, e);
                }
            }

            if let Some(repo) = &self.repository {
                let order_clone = order.clone();
                let repo = repo.clone();
                tokio::spawn(async move {
                    if let Err(e) = repo.save(&order_clone).await {
                        error!("Failed to persist order {}: {}", order_clone.id, e);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::ExecutionService;
    use anyhow::Result;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tokio::sync::mpsc;

    struct MockExecService;
    #[async_trait]
    impl ExecutionService for MockExecService {
        async fn execute(&self, _order: Order) -> Result<()> {
            Ok(())
        }
        async fn get_portfolio(&self) -> Result<Portfolio> {
            Ok(Portfolio::new())
        }
        async fn get_today_orders(&self) -> Result<Vec<Order>> {
            Ok(Vec::new())
        }
    }

    struct FailExecService;
    #[async_trait]
    impl ExecutionService for FailExecService {
        async fn execute(&self, _order: Order) -> Result<()> {
            Err(anyhow::anyhow!("Simulated Failure"))
        }
        async fn get_portfolio(&self) -> Result<Portfolio> {
            Err(anyhow::anyhow!("Simulated Failure"))
        }
        async fn get_today_orders(&self) -> Result<Vec<Order>> {
            Err(anyhow::anyhow!("Simulated Failure"))
        }
    }

    #[tokio::test]
    async fn test_buy_updates_portfolio() {
        let (tx, rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));

        let mut executor = Executor::new(Arc::new(MockExecService), rx, portfolio.clone(), None);
        tokio::spawn(async move { executor.run().await });

        let order = Order {
            id: "1".to_string(),
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(2),
            timestamp: 0,
        };
        tx.send(order).await.unwrap();

        // Allow update
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(800)); // 1000 - (100*2)
        assert_eq!(p.positions.get("ABC").unwrap().quantity, Decimal::from(2));
    }

    #[tokio::test]
    async fn test_failed_execution_does_not_update_portfolio() {
        let (tx, rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));

        let mut executor = Executor::new(Arc::new(FailExecService), rx, portfolio.clone(), None);
        tokio::spawn(async move { executor.run().await });

        let order = Order {
            id: "1".to_string(),
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(2),
            timestamp: 0,
        };
        tx.send(order).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(1000)); // Unchanged
    }
}
