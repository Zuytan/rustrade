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

#[derive(Debug, Clone)]
pub struct ActiveTrailingStop {
    pub stop_order_id: String,
    pub stop_state: crate::application::risk_management::trailing_stops::StopState,
    pub quantity: rust_decimal::Decimal,
    pub trailing_distance: rust_decimal::Decimal,
    pub correlation_id: Option<String>,
}

pub struct ExecutorDependencies {
    pub execution_service: Arc<dyn ExecutionService>,
    pub repository: Option<Arc<dyn TradeRepository>>,
    pub retry_config: RetryConfig,
    pub health_service: Arc<ConnectionHealthService>,
    pub fee_model: Arc<dyn FeeModel>,
    pub agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    pub candle_rx: Option<tokio::sync::broadcast::Receiver<crate::domain::trading::types::Candle>>,
}

pub struct Executor {
    execution_service: Arc<dyn ExecutionService>,
    order_rx: Receiver<Order>,
    portfolio: Arc<RwLock<Portfolio>>,
    repository: Option<Arc<dyn TradeRepository>>,
    order_monitor: Arc<OrderMonitor>,
    health_service: Arc<ConnectionHealthService>,
    fee_model: Arc<dyn FeeModel>,
    agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    candle_rx: Option<tokio::sync::broadcast::Receiver<crate::domain::trading::types::Candle>>,
    active_trailing_stops: Arc<RwLock<std::collections::HashMap<String, ActiveTrailingStop>>>,
}

impl Executor {
    pub fn new(
        order_rx: Receiver<Order>,
        portfolio: Arc<RwLock<Portfolio>>,
        deps: ExecutorDependencies,
    ) -> Self {
        Self {
            execution_service: deps.execution_service,
            order_rx,
            portfolio,
            repository: deps.repository,
            order_monitor: Arc::new(OrderMonitor::new(deps.retry_config)),
            health_service: deps.health_service,
            fee_model: deps.fee_model,
            agent_registry: deps.agent_registry,
            candle_rx: deps.candle_rx,
            active_trailing_stops: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn run(&mut self) {
        info!("Executor started. Running startup reconciliation...");
        if let Err(e) = self.reconcile_on_startup().await {
            error!("Executor: Startup reconciliation failed: {}", e);
        }

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(5));
        let mut candle_rx = self.candle_rx.take();

        // Initial Heartbeat
        self.agent_registry
            .update_heartbeat(
                "Executor",
                crate::application::monitoring::agent_status::HealthStatus::Healthy,
            )
            .await;

        loop {
            tokio::select! {
                Some(order) = self.order_rx.recv() => {
                    self.handle_order(order).await;
                }
                Ok(candle) = async {
                    if let Some(ref mut rx) = candle_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    self.handle_candle(candle).await;
                }
                _ = interval.tick() => {
                    self.check_timeouts().await;
                }
                _ = heartbeat_interval.tick() => {
                    self.agent_registry
                        .update_heartbeat(
                            "Executor",
                            crate::application::monitoring::agent_status::HealthStatus::Healthy,
                        )
                        .await;
                }
            }
        }
    }

    #[instrument(skip(self, order), fields(symbol = %order.symbol, side = ?order.side, qty = %order.quantity, correlation_id = ?order.correlation_id))]
    async fn handle_order(&self, mut order: Order) {
        info!(
            "Executor: Processing Order {}. Symbol: {}, Qty: {}, correlation_id: {:?}",
            order.id, order.symbol, order.quantity, order.correlation_id
        );

        // Cancel trailing stops on Sell order exit
        if order.side == OrderSide::Sell {
            let mut active_stops = self.active_trailing_stops.write().await;
            if let Some(active) = active_stops.remove(&order.symbol) {
                let old_id = active.stop_order_id.clone();
                let symbol = order.symbol.clone();
                let exec = self.execution_service.clone();
                tokio::spawn(async move {
                    if let Err(e) = exec.cancel_order(&old_id, &symbol).await {
                        warn!(
                            "Executor: Failed to cancel trailing stop order {} on sell exit: {}",
                            old_id, e
                        );
                    }
                });
            }
        }

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

        // 1. Execute External (pass by reference)
        match self.execution_service.execute(&order).await {
            Ok(_) => {
                // 2. Retrieve real broker fees (non-blocking, graceful fallback)
                let real_fees = match self.execution_service.get_order_fees(&order.id).await {
                    Ok(fees) => {
                        if let Some(ref f) = fees {
                            info!("Executor: Real broker fees for {}: ${}", order.id, f);
                        }
                        fees
                    }
                    Err(e) => {
                        warn!(
                            "Executor: Fee retrieval failed for {}: {}, using model estimate",
                            order.id, e
                        );
                        None
                    }
                };

                // 3. Update Internal State (Optimistic) with real or estimated fees
                self.update_portfolio(&order, false, real_fees).await;
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

                // Track for retry monitoring if applicable (move order)
                self.order_monitor.track_order(order.clone()).await;

                // 4. Submit initial trailing stop if stop_loss is provided for Buy orders
                if order.side == OrderSide::Buy
                    && let Some(sl_price) = order.stop_loss
                {
                    let trailing_distance = (order.price - sl_price).abs();
                    if trailing_distance > rust_decimal::Decimal::ZERO {
                        let stop_id = uuid::Uuid::new_v4().to_string();
                        let stop_order = Order {
                            id: stop_id.clone(),
                            symbol: order.symbol.clone(),
                            side: OrderSide::Sell,
                            price: sl_price,
                            quantity: order.quantity,
                            order_type: crate::domain::trading::types::OrderType::Stop,
                            status: crate::domain::trading::types::OrderStatus::Pending,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                            correlation_id: order.correlation_id.clone(),
                            stop_loss: None,
                        };

                        let exec = self.execution_service.clone();
                        let stop_order_clone = stop_order.clone();
                        tokio::spawn(async move {
                            if let Err(e) = exec.execute(&stop_order_clone).await {
                                error!(
                                    "Executor: Failed to place initial trailing stop order: {}",
                                    e
                                );
                            }
                        });

                        let mut active_stops = self.active_trailing_stops.write().await;
                        active_stops.insert(order.symbol.clone(), ActiveTrailingStop {
                            stop_order_id: stop_id,
                            stop_state: crate::application::risk_management::trailing_stops::StopState::on_buy(
                                order.price,
                                trailing_distance,
                                rust_decimal::Decimal::ONE,
                            ),
                            quantity: order.quantity,
                            trailing_distance,
                            correlation_id: order.correlation_id.clone(),
                        });
                        info!(
                            "Executor: Initialized trailing stop for {} at stop price {}",
                            order.symbol, sl_price
                        );
                    }
                }
            }
            Err(e) => {
                error!("Executor: Execution failed for {}: {}", order.id, e);
                self.health_service
                    .set_execution_status(
                        ConnectionStatus::Offline,
                        Some(format!("Execution failed: {}", e)),
                    )
                    .await;

                // Update persisted status to 'Rejected' (move order)
                if let Some(repo) = &self.repository {
                    let mut rejected_order = order;
                    rejected_order.status = crate::domain::trading::types::OrderStatus::Rejected;
                    let _ = repo.save(&rejected_order).await;
                }
            }
        }
    }

    async fn handle_candle(&self, candle: crate::domain::trading::types::Candle) {
        let mut active_stops = self.active_trailing_stops.write().await;
        if let Some(active) = active_stops.get_mut(&candle.symbol) {
            let old_stop_price = active.stop_state.get_stop_price();
            let trigger = active.stop_state.on_price_update(
                candle.close,
                active.trailing_distance,
                rust_decimal::Decimal::ONE,
            );

            if trigger.is_some() {
                info!(
                    "Executor: Trailing stop triggered locally for {} at price {}",
                    candle.symbol, candle.close
                );
                active_stops.remove(&candle.symbol);
            } else if let Some(new_stop_price) = active
                .stop_state
                .get_stop_price()
                .filter(|&p| Some(p) != old_stop_price)
            {
                info!(
                    "Executor: Trailing stop price updated for {} from {:?} to {}",
                    candle.symbol, old_stop_price, new_stop_price
                );

                let old_id = active.stop_order_id.clone();
                let symbol = candle.symbol.clone();
                let exec = self.execution_service.clone();

                let new_id = uuid::Uuid::new_v4().to_string();
                let qty = active.quantity;
                let correlation_id = active.correlation_id.clone();

                let new_stop_order = Order {
                    id: new_id.clone(),
                    symbol: symbol.clone(),
                    side: OrderSide::Sell,
                    price: new_stop_price,
                    quantity: qty,
                    order_type: crate::domain::trading::types::OrderType::Stop,
                    status: crate::domain::trading::types::OrderStatus::Pending,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    correlation_id,
                    stop_loss: None,
                };

                let new_id_spawn = new_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = exec.cancel_order(&old_id, &symbol).await {
                        warn!(
                            "Executor: Failed to cancel old trailing stop order {}: {}",
                            old_id, e
                        );
                    }
                    if let Err(e) = exec.execute(&new_stop_order).await {
                        error!(
                            "Executor: Failed to place updated trailing stop order {}: {}",
                            new_id_spawn, e
                        );
                    }
                });

                active.stop_order_id = new_id;
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
    async fn update_portfolio(
        &self,
        order: &Order,
        is_reversal: bool,
        real_fees: Option<rust_decimal::Decimal>,
    ) {
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

        // Use real broker fees if available, otherwise fall back to model estimate
        let fees = if let Some(broker_fees) = real_fees {
            info!(
                "Executor: Using real broker fees: ${} for order {}",
                broker_fees, order.id
            );
            broker_fees
        } else {
            let trade_cost = self
                .fee_model
                .calculate_cost(order.quantity, order.price, order.side);
            let estimated = trade_cost.total_impact;
            info!(
                "Executor: No broker fees, using model estimate: ${} for order {}",
                estimated, order.id
            );
            estimated
        };

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
        async fn execute(&self, _order: &Order) -> Result<()> {
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
        async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<()> {
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

    struct FailExecService;
    #[async_trait]
    impl ExecutionService for FailExecService {
        async fn execute(&self, _order: &Order) -> Result<()> {
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
        async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<()> {
            Err(anyhow::anyhow!("Simulated Failure"))
        }
        async fn cancel_all_orders(&self) -> Result<()> {
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
            rx,
            portfolio.clone(),
            ExecutorDependencies {
                execution_service: Arc::new(MockExecService),
                repository: None,
                retry_config: RetryConfig::default(),
                health_service: Arc::new(ConnectionHealthService::new()),
                fee_model,
                agent_registry: Arc::new(
                    crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                        crate::infrastructure::observability::Metrics::new().unwrap(),
                    ),
                ),
                candle_rx: None,
            },
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
            correlation_id: None,
            stop_loss: None,
        };
        tx.send(order).await.expect("Failed to send order in test");

        // Allow update
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(800)); // 1000 - (100*2) - 0 fees
        assert!(p.positions.contains_key("ABC"), "Position ABC should exist");
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
            rx,
            portfolio.clone(),
            ExecutorDependencies {
                execution_service: Arc::new(FailExecService),
                repository: None,
                retry_config: RetryConfig::default(),
                health_service: Arc::new(ConnectionHealthService::new()),
                fee_model,
                agent_registry: Arc::new(
                    crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                        crate::infrastructure::observability::Metrics::new().unwrap(),
                    ),
                ),
                candle_rx: None,
            },
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
            correlation_id: None,
            stop_loss: None,
        };
        tx.send(order).await.expect("Failed to send order in test");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(1000)); // Unchanged
    }

    struct CaptureExecService {
        executed: Arc<RwLock<Vec<Order>>>,
        cancelled: Arc<RwLock<Vec<String>>>,
    }

    #[async_trait]
    impl ExecutionService for CaptureExecService {
        async fn execute(&self, order: &Order) -> Result<()> {
            self.executed.write().await.push(order.clone());
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
        async fn cancel_order(&self, order_id: &str, _symbol: &str) -> Result<()> {
            self.cancelled.write().await.push(order_id.to_string());
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
    async fn test_executor_trailing_stop_loss() {
        let (tx, rx) = mpsc::channel(10);
        let (candle_tx, candle_rx) = tokio::sync::broadcast::channel(10);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));

        let executed = Arc::new(RwLock::new(Vec::new()));
        let cancelled = Arc::new(RwLock::new(Vec::new()));
        let mock_exec = Arc::new(CaptureExecService {
            executed: executed.clone(),
            cancelled: cancelled.clone(),
        });

        let fee_model = Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO));
        let mut executor = Executor::new(
            rx,
            portfolio.clone(),
            ExecutorDependencies {
                execution_service: mock_exec,
                repository: None,
                retry_config: RetryConfig::default(),
                health_service: Arc::new(ConnectionHealthService::new()),
                fee_model,
                agent_registry: Arc::new(
                    crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                        crate::infrastructure::observability::Metrics::new().unwrap(),
                    ),
                ),
                candle_rx: Some(candle_rx),
            },
        );
        tokio::spawn(async move { executor.run().await });

        // 1. Submit BUY order with stop loss
        let order = Order {
            id: "buy_order_1".to_string(),
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(2),
            order_type: crate::domain::trading::types::OrderType::Limit,
            status: crate::domain::trading::types::OrderStatus::New,
            timestamp: 0,
            correlation_id: None,
            stop_loss: Some(Decimal::from(90)),
        };
        tx.send(order).await.expect("Failed to send order");

        // Allow execution
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify that initial BUY and the initial Stop order were submitted
        let execs = executed.read().await;
        assert_eq!(execs.len(), 2);
        assert_eq!(execs[0].id, "buy_order_1");
        assert_eq!(
            execs[1].order_type,
            crate::domain::trading::types::OrderType::Stop
        );
        assert_eq!(execs[1].price, Decimal::from(90));
        assert_eq!(execs[1].side, OrderSide::Sell);
        let initial_stop_id = execs[1].id.clone();
        drop(execs);

        // 2. Send candle rising to 110 -> Stop should raise to 100
        let candle = crate::domain::trading::types::Candle {
            symbol: "ABC".to_string(),
            open: Decimal::from(100),
            high: Decimal::from(110),
            low: Decimal::from(100),
            close: Decimal::from(110),
            volume: Decimal::from(100),
            timestamp: 0,
        };
        candle_tx.send(candle).expect("Failed to send candle");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify old stop order cancelled and new one placed
        let cancels = cancelled.read().await;
        assert_eq!(cancels.len(), 1);
        assert_eq!(cancels[0], initial_stop_id);

        let execs = executed.read().await;
        assert_eq!(execs.len(), 3);
        assert_eq!(
            execs[2].order_type,
            crate::domain::trading::types::OrderType::Stop
        );
        assert_eq!(execs[2].price, Decimal::from(100)); // 110 - 10 trailing distance
    }
}
