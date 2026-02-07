use crate::application::monitoring::connection_health_service::{
    ConnectionHealthService, ConnectionStatus,
};
use crate::application::risk_management::{
    order_monitor::{MonitorAction, OrderMonitor},
    order_retry_strategy::RetryConfig,
};
use crate::domain::ports::ExecutionService;
use crate::domain::repositories::TradeRepository;
use crate::domain::trading::fee_model::FeeModel;
use crate::domain::trading::portfolio::{Portfolio, Position};
use crate::domain::trading::types::{Order, OrderSide};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tracing::{error, info, instrument, warn};

pub struct Executor {
    execution_service: Arc<dyn ExecutionService>,
    order_rx: Receiver<Order>,
    portfolio: Arc<RwLock<Portfolio>>,
    repository: Option<Arc<dyn TradeRepository>>,
    order_monitor: Arc<OrderMonitor>,
    health_service: Arc<ConnectionHealthService>,
    fee_model: Arc<dyn FeeModel>,
}

impl Executor {
    pub fn new(
        execution_service: Arc<dyn ExecutionService>,
        order_rx: Receiver<Order>,
        portfolio: Arc<RwLock<Portfolio>>,
        repository: Option<Arc<dyn TradeRepository>>,
        retry_config: RetryConfig,
        health_service: Arc<ConnectionHealthService>,
        fee_model: Arc<dyn FeeModel>,
    ) -> Self {
        Self {
            execution_service,
            order_rx,
            portfolio,
            repository,
            order_monitor: Arc::new(OrderMonitor::new(retry_config)),
            health_service,
            fee_model,
        }
    }

    pub async fn run(&mut self) {
        info!("Executor started. Running startup reconciliation...");
        if let Err(e) = self.reconcile_on_startup().await {
            error!("Executor: Startup reconciliation failed: {}", e);
        }

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(order) = self.order_rx.recv() => {
                    self.handle_order(order).await;
                }
                _ = interval.tick() => {
                    self.check_timeouts().await;
                }
            }
        }
    }

    #[instrument(skip(self, order), fields(symbol = %order.symbol, side = ?order.side, qty = %order.quantity))]
    async fn handle_order(&self, mut order: Order) {
        info!(
            "Executor: Processing Order {}. Symbol: {}, Qty: {}",
            order.id, order.symbol, order.quantity
        );

        // 0. IDEMPOTENCY: Persist with 'Pending' status BEFORE execution
        order.status = crate::domain::trading::types::OrderStatus::Pending;
        if let Some(repo) = &self.repository
            && let Err(e) = repo.save(&order).await
        {
            error!(
                "Executor: IDEMPOTENCY SAFETY - Failed to pre-persist order {}: {}. ABORTING execution to prevent potential double-spend.",
                order.id, e
            );
            return;
        }

        // 1. Execute External
        match self.execution_service.execute(order.clone()).await {
            Ok(_) => {
                // Track for retry monitoring if applicable
                self.order_monitor.track_order(order.clone()).await;

                // 2. Update Internal State (Optimistic)
                self.update_portfolio(&order, false).await;
                info!("Executor: Order {} sent to exchange.", order.id);

                // Update persisted status to 'New' (now officially on the exchange)
                if let Some(repo) = &self.repository {
                    let mut submitted_order = order.clone();
                    submitted_order.status = crate::domain::trading::types::OrderStatus::New;
                    let _ = repo.save(&submitted_order).await;
                }

                // Broadcast Online if it was offline
                self.health_service
                    .set_execution_status(ConnectionStatus::Online, None)
                    .await;
            }
            Err(e) => {
                error!("Executor: Execution failed for {}: {}", order.id, e);
                self.health_service
                    .set_execution_status(
                        ConnectionStatus::Offline,
                        Some(format!("Execution failed: {}", e)),
                    )
                    .await;

                // Update persisted status to 'Rejected'
                if let Some(repo) = &self.repository {
                    let mut rejected_order = order.clone();
                    rejected_order.status = crate::domain::trading::types::OrderStatus::Rejected;
                    let _ = repo.save(&rejected_order).await;
                }
            }
        }
    }

    async fn check_timeouts(&self) {
        let actions = self.order_monitor.check_timeouts().await;
        for action in actions {
            match action {
                MonitorAction::None => {}
                _ => { /* Already handled in check_timeouts or similar? */ }
            }
        }
    }

    /// Startup task to sync locally pending orders with exchange state
    #[instrument(skip(self))]
    async fn reconcile_on_startup(&self) -> Result<()> {
        let repo = match &self.repository {
            Some(r) => r,
            None => return Ok(()),
        };

        // 1. Fetch locally pending orders
        let local_pending = repo
            .find_by_status(crate::domain::trading::types::OrderStatus::Pending)
            .await?;
        if local_pending.is_empty() {
            info!("Executor: No pending orders to reconcile.");
            return Ok(());
        }

        info!(
            "Executor: Found {} pending orders. Synchronizing with exchange...",
            local_pending.is_empty()
        );

        // 2. Fetch exchange orders (Open and Today's)
        let open_orders = self.execution_service.get_open_orders().await?;
        let today_orders = self.execution_service.get_today_orders().await?;

        // 3. Reconcile
        for mut order in local_pending {
            // Check if order ID exists in any exchange list
            let on_exchange = open_orders.iter().any(|o| o.id == order.id)
                || today_orders.iter().any(|o| o.id == order.id);

            if on_exchange {
                info!(
                    "Executor: Order {} found on exchange. Marking as 'New' (confirmed).",
                    order.id
                );
                order.status = crate::domain::trading::types::OrderStatus::New;
                let _ = repo.save(&order).await;
            } else {
                // Not found: assumed never reached the exchange
                warn!(
                    "Executor: Pending order {} NOT found on exchange. Marking as 'Rejected' (failed safety).",
                    order.id
                );
                order.status = crate::domain::trading::types::OrderStatus::Rejected;
                let _ = repo.save(&order).await;
            }
        }

        info!("Executor: Startup reconciliation complete.");
        Ok(())
    }

    #[instrument(skip(self, order), fields(symbol = %order.symbol, side = ?order.side))]
    async fn update_portfolio(&self, order: &Order, is_reversal: bool) {
        let mut portfolio =
            match tokio::time::timeout(std::time::Duration::from_secs(2), self.portfolio.write())
                .await
            {
                Ok(guard) => guard,
                Err(_) => {
                    error!("Executor: Deadlock detected acquiring Portfolio write lock");
                    return;
                }
            };

        let cost = order.price * order.quantity;
        let trade_cost = self
            .fee_model
            .calculate_cost(order.quantity, order.price, order.side);
        let fees = trade_cost.total_impact;

        match order.side {
            OrderSide::Buy => {
                // If reversal (Buy), we ADD cash and fees back. If normal (Buy), we SUBTRACT cost + fees.
                if is_reversal {
                    portfolio.cash += cost + fees;
                } else {
                    portfolio.cash -= cost + fees;
                }

                let position =
                    portfolio
                        .positions
                        .entry(order.symbol.clone())
                        .or_insert(Position {
                            symbol: order.symbol.clone(),
                            quantity: rust_decimal::Decimal::ZERO,
                            average_price: rust_decimal::Decimal::ZERO,
                        });

                // Update position logic is complex for reversal of average price
                // For simplicity in this fix, we primarily care about Quantity and Cash
                if is_reversal {
                    position.quantity -= order.quantity;
                    // NOTE: Average price reversal is lossy if we don't store history.
                    // Accepting this limitation for "blind" optimistic updates.
                } else {
                    let total_val = (position.quantity * position.average_price) + cost;
                    let new_qty = position.quantity + order.quantity;
                    if !new_qty.is_zero() {
                        position.average_price = total_val / new_qty;
                    }
                    position.quantity = new_qty;
                }
            }
            OrderSide::Sell => {
                if is_reversal {
                    portfolio.cash -= cost - fees;
                    if let Some(position) = portfolio.positions.get_mut(&order.symbol) {
                        position.quantity += order.quantity;
                    }
                } else {
                    portfolio.cash += cost - fees;
                    if let Some(position) = portfolio.positions.get_mut(&order.symbol) {
                        position.quantity -= order.quantity;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::{ExecutionService, OrderUpdate};
    use crate::domain::trading::fee_model::ConstantFeeModel;
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
        async fn get_open_orders(&self) -> Result<Vec<Order>> {
            Ok(Vec::new())
        }
        async fn cancel_order(&self, _order_id: &str) -> Result<()> {
            Ok(())
        }
        async fn subscribe_order_updates(
            &self,
        ) -> Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
            let (_tx, rx) = tokio::sync::broadcast::channel(1);

            Ok(rx)
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
        async fn get_open_orders(&self) -> Result<Vec<Order>> {
            Err(anyhow::anyhow!("Simulated Failure"))
        }
        async fn cancel_order(&self, _order_id: &str) -> Result<()> {
            Err(anyhow::anyhow!("Simulated Failure"))
        }
        async fn subscribe_order_updates(
            &self,
        ) -> Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
            Err(anyhow::anyhow!("Simulated Failure"))
        }
    }

    #[tokio::test]
    async fn test_buy_updates_portfolio() {
        let (tx, rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));

        let fee_model = Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO));
        let mut executor = Executor::new(
            Arc::new(MockExecService),
            rx,
            portfolio.clone(),
            None,
            RetryConfig::default(),
            Arc::new(ConnectionHealthService::new()),
            fee_model,
        );
        tokio::spawn(async move { executor.run().await });

        let order = Order {
            id: "1".to_string(),
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(2),
            order_type: crate::domain::trading::types::OrderType::Limit,
            status: crate::domain::trading::types::OrderStatus::New,
            timestamp: 0,
        };
        tx.send(order).await.unwrap();

        // Allow update
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(800)); // 1000 - (100*2) - 0 fees
        assert_eq!(p.positions.get("ABC").unwrap().quantity, Decimal::from(2));
    }

    #[tokio::test]
    async fn test_failed_execution_does_not_update_portfolio() {
        let (tx, rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));

        let fee_model = Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO));
        let mut executor = Executor::new(
            Arc::new(FailExecService),
            rx,
            portfolio.clone(),
            None,
            RetryConfig::default(),
            Arc::new(ConnectionHealthService::new()),
            fee_model,
        );
        tokio::spawn(async move { executor.run().await });

        let order = Order {
            id: "1".to_string(),
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(2),
            order_type: crate::domain::trading::types::OrderType::Limit,
            status: crate::domain::trading::types::OrderStatus::New,
            timestamp: 0,
        };
        tx.send(order).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(1000)); // Unchanged
    }
}
