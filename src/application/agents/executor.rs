use crate::application::monitoring::agent_status::HealthStatus;
use crate::application::monitoring::connection_health_service::{
    ConnectionHealthService, ConnectionStatus,
};
use crate::application::risk_management::{
    order_monitor::{MonitorAction, OrderMonitor},
    order_retry_strategy::RetryConfig,
};
use crate::domain::errors::OrderSubmissionUnknown;
use crate::domain::ports::ExecutionService;
use crate::domain::repositories::TradeRepository;
use crate::domain::trading::fee_model::FeeModel;
use crate::domain::trading::portfolio::{Portfolio, Position};
use crate::domain::trading::types::{Order, OrderSide, OrderStatus};
use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tracing::{error, info, instrument, warn};

/// A pending order the broker does not know is only rejected after this age (ms),
/// since a just-submitted order may not be visible at the broker yet.
const RECONCILE_GRACE_MS: i64 = 30_000;
/// First delay before retrying a failed reconciliation (doubled up to the max).
const RECONCILE_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
const RECONCILE_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);

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
    /// False until pending orders have been reconciled with the broker.
    /// While false, incoming orders are refused (fail-closed).
    reconciled: AtomicBool,
    /// Orders submitted in this session whose outcome is unknown, kept in full
    /// (stop_loss is not persisted) until reconciliation resolves them.
    unknown_orders: RwLock<std::collections::HashMap<String, Order>>,
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
            reconciled: AtomicBool::new(false),
            unknown_orders: RwLock::new(std::collections::HashMap::new()),
        }
    }

    fn is_reconciled(&self) -> bool {
        self.reconciled.load(Ordering::SeqCst)
    }

    /// Runs a reconciliation pass and records whether it fully succeeded.
    async fn try_reconcile(&self) -> bool {
        match self.reconcile_pending().await {
            Ok(_) => {
                if !self.is_reconciled() {
                    info!("Executor: Reconciliation complete. Order processing enabled.");
                }
                self.reconciled.store(true, Ordering::SeqCst);
                true
            }
            Err(e) => {
                error!(
                    "Executor: Reconciliation failed: {}. Incoming orders are refused until it succeeds.",
                    e
                );
                self.reconciled.store(false, Ordering::SeqCst);
                self.health_service
                    .set_execution_status(
                        ConnectionStatus::Offline,
                        Some(format!("Order reconciliation failed: {}", e)),
                    )
                    .await;
                false
            }
        }
    }

    pub async fn run(&mut self) {
        info!("Executor started. Running startup reconciliation...");
        self.try_reconcile().await;
        let mut reconcile_backoff = RECONCILE_INITIAL_BACKOFF;
        let mut next_reconcile_retry = tokio::time::Instant::now() + reconcile_backoff;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(5));
        let mut candle_rx = self.candle_rx.take();

        // Initial Heartbeat
        self.agent_registry
            .update_heartbeat("Executor", self.heartbeat_status())
            .await;
        self.publish_reconciliation_metrics().await;

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
                    if self.is_reconciled() {
                        // Periodic pass: resolves orders left Pending by ambiguous submissions.
                        if let Err(e) = self.reconcile_pending().await {
                            warn!("Executor: Periodic reconciliation incomplete: {}", e);
                        }
                    } else if tokio::time::Instant::now() >= next_reconcile_retry {
                        if self.try_reconcile().await {
                            reconcile_backoff = RECONCILE_INITIAL_BACKOFF;
                        } else {
                            reconcile_backoff = (reconcile_backoff * 2).min(RECONCILE_MAX_BACKOFF);
                            next_reconcile_retry = tokio::time::Instant::now() + reconcile_backoff;
                        }
                    }
                    self.agent_registry
                        .update_heartbeat("Executor", self.heartbeat_status())
                        .await;
                    self.publish_reconciliation_metrics().await;
                }
            }
        }
    }

    /// Exports reconciliation state for alerting (`rustrade_executor_reconciled`,
    /// `rustrade_orders_unknown_outcome`).
    async fn publish_reconciliation_metrics(&self) {
        let metrics = self.agent_registry.metrics();
        metrics
            .executor_reconciled
            .set(if self.is_reconciled() { 1.0 } else { 0.0 });
        metrics
            .orders_unknown_outcome
            .set(self.unknown_orders.read().await.len() as f64);
    }

    fn heartbeat_status(&self) -> HealthStatus {
        if self.is_reconciled() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Degraded
        }
    }

    #[instrument(skip(self, order), fields(symbol = %order.symbol, side = ?order.side, qty = %order.quantity, correlation_id = ?order.correlation_id))]
    async fn handle_order(&self, mut order: Order) {
        info!(
            "Executor: Processing Order {}. Symbol: {}, Qty: {}, correlation_id: {:?}",
            order.id, order.symbol, order.quantity, order.correlation_id
        );

        // Fail-closed: never open new exposure while the broker state of pending orders is
        // unknown. Sells are let through: they reduce risk (exits, stop-losses), and the
        // broker rejects selling a position that does not exist.
        if !self.is_reconciled() && order.side == OrderSide::Sell {
            warn!(
                "Executor: Processing exit order {} for {} although pending orders are not reconciled.",
                order.id, order.symbol
            );
        } else if !self.is_reconciled() {
            error!(
                "Executor: REFUSING order {} for {}: pending orders are not reconciled with the broker.",
                order.id, order.symbol
            );
            if let Some(repo) = &self.repository {
                order.status = OrderStatus::Rejected;
                if let Err(e) = repo.save(&order).await {
                    error!(
                        "Executor: Failed to persist refused order {}: {}",
                        order.id, e
                    );
                }
            }
            return;
        }

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
            Ok(_) => self.on_order_accepted(&order, true).await,
            Err(e) if e.downcast_ref::<OrderSubmissionUnknown>().is_some() => {
                // The order may be live at the broker: keep it Pending for reconciliation,
                // and do not touch the portfolio until its outcome is known.
                error!(
                    "Executor: Order {} for {} has an UNKNOWN outcome at the broker: {}. Kept Pending for reconciliation.",
                    order.id, order.symbol, e
                );
                // Keep the full order (incl. stop_loss, not persisted) so that its accepted
                // path, trailing stop included, runs once reconciliation confirms it.
                // Without a repository nothing is reconciled, so nothing would ever remove it.
                if self.repository.is_some() {
                    self.unknown_orders
                        .write()
                        .await
                        .insert(order.id.clone(), order.clone());
                }
                self.health_service
                    .set_execution_status(
                        ConnectionStatus::Offline,
                        Some(format!("Order outcome unknown: {}", e)),
                    )
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

                // Update persisted status to 'Rejected' (move order)
                if let Some(repo) = &self.repository {
                    let mut rejected_order = order;
                    rejected_order.status = crate::domain::trading::types::OrderStatus::Rejected;
                    if let Err(save_err) = repo.save(&rejected_order).await {
                        error!(
                            "Executor: Failed to persist rejected order {}: {}",
                            rejected_order.id, save_err
                        );
                    }
                }
            }
        }
    }

    /// Post-submission path for an order the broker accepted: fees, optimistic portfolio
    /// update, persisted as New, retry tracking (if `track_for_retry`) and initial
    /// trailing stop.
    async fn on_order_accepted(&self, order: &Order, track_for_retry: bool) {
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
        self.update_portfolio(order, false, real_fees).await;
        info!("Executor: Order {} sent to exchange.", order.id);

        // Update persisted status to 'New' (now officially on the exchange)
        if let Some(repo) = &self.repository {
            let mut submitted_order = order.clone();
            submitted_order.status = crate::domain::trading::types::OrderStatus::New;
            if let Err(e) = repo.save(&submitted_order).await {
                error!(
                    "Executor: Failed to persist submitted order {} as New: {}",
                    order.id, e
                );
            }
        }

        // Broadcast Online if it was offline
        self.health_service
            .set_execution_status(ConnectionStatus::Online, None)
            .await;

        // Track for retry monitoring if applicable (an order already over is not tracked)
        if track_for_retry {
            self.order_monitor.track_order(order.clone()).await;
        }

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
                active_stops.insert(
                    order.symbol.clone(),
                    ActiveTrailingStop {
                        stop_order_id: stop_id,
                        stop_state:
                            crate::application::risk_management::trailing_stops::StopState::on_buy(
                                order.price,
                                trailing_distance,
                                rust_decimal::Decimal::ONE,
                            ),
                        quantity: order.quantity,
                        trailing_distance,
                        correlation_id: order.correlation_id.clone(),
                    },
                );
                info!(
                    "Executor: Initialized trailing stop for {} at stop price {}",
                    order.symbol, sl_price
                );
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

    /// Syncs locally Pending orders with the broker, looking each one up by client order id.
    ///
    /// - Found → its broker status is persisted.
    /// - Confirmed absent and older than `RECONCILE_GRACE_MS` → Rejected.
    /// - Confirmed absent but younger → left Pending (may not be visible yet).
    /// - Lookup failure → left Pending; the first error is returned after all orders
    ///   have been tried. Persistence failures are returned too.
    ///
    /// Returns the number of orders whose status was resolved.
    #[instrument(skip(self))]
    async fn reconcile_pending(&self) -> Result<usize> {
        let repo = match &self.repository {
            Some(r) => r,
            None => return Ok(0),
        };

        let local_pending = repo.find_by_status(OrderStatus::Pending).await?;
        if local_pending.is_empty() {
            return Ok(0);
        }

        info!(
            "Executor: Found {} pending orders. Synchronizing with exchange...",
            local_pending.len()
        );

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut resolved = 0;
        let mut first_error: Option<anyhow::Error> = None;

        for mut order in local_pending {
            match self
                .execution_service
                .find_order_status(&order.id, &order.symbol)
                .await
            {
                Ok(Some(state)) => {
                    let status = state.status;
                    info!(
                        "Executor: Pending order {} found at broker with status {:?} (filled {}).",
                        order.id, status, state.filled_qty
                    );
                    let submitted_this_session =
                        self.unknown_orders.write().await.remove(&order.id);
                    if !state.is_dead_without_fill() {
                        match submitted_this_session {
                            // Run the accepted path that was skipped: portfolio update,
                            // retry tracking and trailing stop.
                            Some(mut full_order) => {
                                if state.is_terminal_partial_fill() {
                                    // Canceled/expired after a partial fill: book only
                                    // what was actually executed.
                                    warn!(
                                        "Executor: Order {} for {} ended {:?} after a partial fill of {} (ordered {}).",
                                        order.id,
                                        order.symbol,
                                        status,
                                        state.filled_qty,
                                        full_order.quantity
                                    );
                                    full_order.quantity = state.filled_qty;
                                }
                                let still_working = !status.is_terminal_without_fill()
                                    && status != OrderStatus::Filled;
                                self.on_order_accepted(&full_order, still_working).await;
                            }
                            None => error!(
                                "Executor: Order {} for {} ({:?}, filled {}) comes from a previous run: no trailing stop could be placed (stop loss is not persisted).",
                                order.id, order.symbol, status, state.filled_qty
                            ),
                        }
                    }
                    order.status = status;
                    repo.save(&order).await?;
                    resolved += 1;
                }
                Ok(None) if now_ms - order.timestamp >= RECONCILE_GRACE_MS => {
                    self.unknown_orders.write().await.remove(&order.id);
                    warn!(
                        "Executor: Pending order {} NOT found at broker. Marking as Rejected.",
                        order.id
                    );
                    order.status = OrderStatus::Rejected;
                    repo.save(&order).await?;
                    resolved += 1;
                }
                Ok(None) => {
                    info!(
                        "Executor: Pending order {} not visible at broker yet. Retrying later.",
                        order.id
                    );
                }
                Err(e) => {
                    warn!(
                        "Executor: Could not look up pending order {}: {}",
                        order.id, e
                    );
                    if first_error.is_none() {
                        first_error = Some(e.context(format!("lookup of order {}", order.id)));
                    }
                }
            }
        }

        match first_error {
            Some(e) => Err(e),
            None => Ok(resolved),
        }
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

    // ===== Reconciliation & ambiguous submission tests =====

    use crate::domain::errors::OrderSubmissionUnknown;
    use crate::domain::ports::BrokerOrderState;
    use crate::domain::trading::types::OrderStatus;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    /// Repository that upserts by order id (like the SQLite repository).
    #[derive(Default)]
    struct UpsertRepo {
        orders: Mutex<HashMap<String, Order>>,
        fail_saves: AtomicBool,
    }

    impl UpsertRepo {
        fn with_orders(orders: Vec<Order>) -> Self {
            let repo = Self::default();
            for o in orders {
                repo.orders
                    .lock()
                    .expect("repo lock")
                    .insert(o.id.clone(), o);
            }
            repo
        }

        fn status_of(&self, id: &str) -> Option<OrderStatus> {
            self.orders
                .lock()
                .expect("repo lock")
                .get(id)
                .map(|o| o.status)
        }
    }

    #[async_trait]
    impl TradeRepository for UpsertRepo {
        async fn save(&self, trade: &Order) -> Result<()> {
            if self.fail_saves.load(Ordering::SeqCst) {
                anyhow::bail!("disk full");
            }
            self.orders
                .lock()
                .expect("repo lock")
                .insert(trade.id.clone(), trade.clone());
            Ok(())
        }
        async fn find_by_symbol(&self, symbol: &str) -> Result<Vec<Order>> {
            let orders = self.orders.lock().expect("repo lock");
            Ok(orders
                .values()
                .filter(|o| o.symbol == symbol)
                .cloned()
                .collect())
        }
        async fn find_by_status(&self, status: OrderStatus) -> Result<Vec<Order>> {
            let orders = self.orders.lock().expect("repo lock");
            Ok(orders
                .values()
                .filter(|o| o.status == status)
                .cloned()
                .collect())
        }
        async fn find_recent(&self, _limit: usize) -> Result<Vec<Order>> {
            self.get_all().await
        }
        async fn get_all(&self) -> Result<Vec<Order>> {
            Ok(self
                .orders
                .lock()
                .expect("repo lock")
                .values()
                .cloned()
                .collect())
        }
        async fn count(&self) -> Result<usize> {
            Ok(self.orders.lock().expect("repo lock").len())
        }
    }

    type Lookup = fn(&str) -> Result<Option<BrokerOrderState>>;

    /// Broker state with the given executed quantity.
    fn st(status: OrderStatus, filled_qty: u32) -> Option<BrokerOrderState> {
        Some(BrokerOrderState {
            status,
            filled_qty: Decimal::from(filled_qty),
        })
    }

    /// Execution service with scripted `execute` and `find_order_status` behavior.
    struct ScriptedExecService {
        execute_unknown: bool,
        lookup: Lookup,
        execute_calls: AtomicUsize,
    }

    impl ScriptedExecService {
        fn new(execute_unknown: bool, lookup: Lookup) -> Self {
            Self {
                execute_unknown,
                lookup,
                execute_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ExecutionService for ScriptedExecService {
        async fn execute(&self, order: &Order) -> Result<()> {
            self.execute_calls.fetch_add(1, Ordering::SeqCst);
            if self.execute_unknown {
                return Err(OrderSubmissionUnknown {
                    client_order_id: order.id.clone(),
                    reason: "timeout".to_string(),
                }
                .into());
            }
            Ok(())
        }
        async fn get_portfolio(&self) -> Result<Portfolio> {
            Ok(Portfolio::new())
        }
        async fn get_today_orders(&self) -> Result<Vec<Order>> {
            anyhow::bail!("reconciliation must not rely on today's orders")
        }
        async fn get_open_orders(&self) -> Result<Vec<Order>> {
            anyhow::bail!("reconciliation must not rely on open orders")
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
        async fn find_order_status(
            &self,
            client_order_id: &str,
            _symbol: &str,
        ) -> Result<Option<BrokerOrderState>> {
            (self.lookup)(client_order_id)
        }
    }

    fn pending_order(id: &str, timestamp_ms: i64) -> Order {
        Order {
            id: id.to_string(),
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(2),
            order_type: crate::domain::trading::types::OrderType::Limit,
            status: OrderStatus::Pending,
            timestamp: timestamp_ms,
            correlation_id: None,
            stop_loss: None,
        }
    }

    fn executor_with(
        exec: Arc<ScriptedExecService>,
        repo: Arc<UpsertRepo>,
        portfolio: Arc<RwLock<Portfolio>>,
    ) -> (Executor, mpsc::Sender<Order>) {
        let (tx, rx) = mpsc::channel(10);
        let executor = Executor::new(
            rx,
            portfolio,
            ExecutorDependencies {
                execution_service: exec,
                repository: Some(repo),
                retry_config: RetryConfig::default(),
                health_service: Arc::new(ConnectionHealthService::new()),
                fee_model: Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO)),
                agent_registry: Arc::new(
                    crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                        crate::infrastructure::observability::Metrics::new().expect("metrics"),
                    ),
                ),
                candle_rx: None,
            },
        );
        (executor, tx)
    }

    fn old_ts() -> i64 {
        chrono::Utc::now().timestamp_millis() - 10 * RECONCILE_GRACE_MS
    }

    #[tokio::test]
    async fn test_reconcile_applies_broker_status_and_rejects_missing_old_orders() {
        let repo = Arc::new(UpsertRepo::with_orders(vec![
            pending_order("filled", old_ts()),
            pending_order("missing", old_ts()),
        ]));
        let exec = Arc::new(ScriptedExecService::new(false, |id| match id {
            "filled" => Ok(st(OrderStatus::Filled, 2)),
            _ => Ok(None),
        }));
        let (executor, _tx) =
            executor_with(exec, repo.clone(), Arc::new(RwLock::new(Portfolio::new())));

        let resolved = executor.reconcile_pending().await.expect("reconcile ok");
        assert_eq!(resolved, 2);
        assert_eq!(repo.status_of("filled"), Some(OrderStatus::Filled));
        assert_eq!(repo.status_of("missing"), Some(OrderStatus::Rejected));
    }

    #[tokio::test]
    async fn test_reconcile_keeps_young_missing_order_pending() {
        let now = chrono::Utc::now().timestamp_millis();
        let repo = Arc::new(UpsertRepo::with_orders(vec![pending_order("young", now)]));
        let exec = Arc::new(ScriptedExecService::new(false, |_| Ok(None)));
        let (executor, _tx) =
            executor_with(exec, repo.clone(), Arc::new(RwLock::new(Portfolio::new())));

        let resolved = executor.reconcile_pending().await.expect("reconcile ok");
        assert_eq!(resolved, 0);
        assert_eq!(repo.status_of("young"), Some(OrderStatus::Pending));
    }

    #[tokio::test]
    async fn test_reconcile_lookup_failure_keeps_order_pending_and_errors() {
        let repo = Arc::new(UpsertRepo::with_orders(vec![
            pending_order("unreachable", old_ts()),
            pending_order("filled", old_ts()),
        ]));
        let exec = Arc::new(ScriptedExecService::new(false, |id| match id {
            "filled" => Ok(st(OrderStatus::Filled, 2)),
            _ => anyhow::bail!("broker unreachable"),
        }));
        let (executor, _tx) =
            executor_with(exec, repo.clone(), Arc::new(RwLock::new(Portfolio::new())));

        assert!(executor.reconcile_pending().await.is_err());
        assert_eq!(repo.status_of("unreachable"), Some(OrderStatus::Pending));
        // Other orders are still reconciled.
        assert_eq!(repo.status_of("filled"), Some(OrderStatus::Filled));
    }

    #[tokio::test]
    async fn test_reconcile_propagates_save_failure() {
        let repo = Arc::new(UpsertRepo::with_orders(vec![pending_order("o1", old_ts())]));
        repo.fail_saves.store(true, Ordering::SeqCst);
        let exec = Arc::new(ScriptedExecService::new(false, |_| {
            Ok(st(OrderStatus::Filled, 2))
        }));
        let (executor, _tx) = executor_with(exec, repo, Arc::new(RwLock::new(Portfolio::new())));

        assert!(executor.reconcile_pending().await.is_err());
    }

    #[tokio::test]
    async fn test_orders_refused_until_reconciled() {
        let repo = Arc::new(UpsertRepo::with_orders(vec![pending_order(
            "stuck",
            old_ts(),
        )]));
        let exec = Arc::new(ScriptedExecService::new(false, |id| match id {
            "stuck" => anyhow::bail!("broker unreachable"),
            _ => Ok(None),
        }));
        let portfolio = Arc::new(RwLock::new(Portfolio::new()));
        let (mut executor, tx) = executor_with(exec.clone(), repo.clone(), portfolio);
        tokio::spawn(async move { executor.run().await });

        let mut order = pending_order("new_order", 0);
        order.status = OrderStatus::New;
        tx.send(order).await.expect("send order");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repo.status_of("new_order"), Some(OrderStatus::Rejected));
        assert_eq!(repo.status_of("stuck"), Some(OrderStatus::Pending));

        // Exits stay possible: a Sell is executed even while unreconciled.
        let mut exit = pending_order("exit_order", chrono::Utc::now().timestamp_millis());
        exit.side = OrderSide::Sell;
        exit.status = OrderStatus::New;
        tx.send(exit).await.expect("send exit");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 1);
        assert_eq!(repo.status_of("exit_order"), Some(OrderStatus::New));
    }

    #[tokio::test]
    async fn test_unknown_submission_keeps_order_pending_and_portfolio_unchanged() {
        let repo = Arc::new(UpsertRepo::default());
        let exec = Arc::new(ScriptedExecService::new(true, |_| Ok(None)));
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));
        let (mut executor, tx) = executor_with(exec.clone(), repo.clone(), portfolio.clone());
        tokio::spawn(async move { executor.run().await });

        // Fresh timestamp: within the grace period, the periodic pass must not reject it.
        let mut order = pending_order("ambiguous", chrono::Utc::now().timestamp_millis());
        order.status = OrderStatus::New;
        tx.send(order).await.expect("send order");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 1);
        assert_eq!(repo.status_of("ambiguous"), Some(OrderStatus::Pending));
        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(1000));
        assert!(!p.positions.contains_key("ABC"));
    }

    #[tokio::test]
    async fn test_unknown_buy_confirmed_filled_updates_portfolio_and_places_stop() {
        let repo = Arc::new(UpsertRepo::default());
        let exec = Arc::new(ScriptedExecService::new(true, |id| match id {
            "ambiguous_buy" => Ok(st(OrderStatus::Filled, 2)),
            _ => Ok(None),
        }));
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));
        let (executor, _tx) = executor_with(exec.clone(), repo.clone(), portfolio.clone());
        assert!(executor.try_reconcile().await);

        let mut order = pending_order("ambiguous_buy", chrono::Utc::now().timestamp_millis());
        order.stop_loss = Some(Decimal::from(90));
        executor.handle_order(order).await;
        assert_eq!(repo.status_of("ambiguous_buy"), Some(OrderStatus::Pending));
        assert!(executor.active_trailing_stops.read().await.is_empty());

        // Reconciliation confirms the fill: the skipped accepted path now runs.
        assert_eq!(executor.reconcile_pending().await.expect("reconcile"), 1);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        assert_eq!(repo.status_of("ambiguous_buy"), Some(OrderStatus::Filled));
        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(800)); // 1000 - 100 * 2
        assert!(p.positions.contains_key("ABC"));
        drop(p);
        assert!(
            executor
                .active_trailing_stops
                .read()
                .await
                .contains_key("ABC")
        );
        // Buy + initial stop order submission
        assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 2);
        assert!(executor.unknown_orders.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_unknown_order_confirmed_rejected_leaves_portfolio_unchanged() {
        let repo = Arc::new(UpsertRepo::default());
        let exec = Arc::new(ScriptedExecService::new(true, |_| {
            Ok(st(OrderStatus::Rejected, 0))
        }));
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));
        let (executor, _tx) = executor_with(exec, repo.clone(), portfolio.clone());
        assert!(executor.try_reconcile().await);

        let mut order = pending_order("ambiguous_buy", chrono::Utc::now().timestamp_millis());
        order.stop_loss = Some(Decimal::from(90));
        executor.handle_order(order).await;
        executor.reconcile_pending().await.expect("reconcile");

        assert_eq!(repo.status_of("ambiguous_buy"), Some(OrderStatus::Rejected));
        assert_eq!(portfolio.read().await.cash, Decimal::from(1000));
        assert!(executor.active_trailing_stops.read().await.is_empty());
        assert!(executor.unknown_orders.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_unknown_buy_canceled_after_partial_fill_books_filled_quantity() {
        let repo = Arc::new(UpsertRepo::default());
        let exec = Arc::new(ScriptedExecService::new(true, |_| {
            Ok(st(OrderStatus::Canceled, 1))
        }));
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));
        let (executor, _tx) = executor_with(exec, repo.clone(), portfolio.clone());
        assert!(executor.try_reconcile().await);

        let mut order = pending_order("partial", chrono::Utc::now().timestamp_millis());
        order.stop_loss = Some(Decimal::from(90));
        executor.handle_order(order).await;
        executor.reconcile_pending().await.expect("reconcile");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // 1 of 2 executed: only that quantity is booked and protected.
        assert_eq!(repo.status_of("partial"), Some(OrderStatus::Canceled));
        let p = portfolio.read().await;
        assert_eq!(p.cash, Decimal::from(900));
        assert_eq!(
            p.positions.get("ABC").map(|pos| pos.quantity),
            Some(Decimal::from(1))
        );
        drop(p);
        let stops = executor.active_trailing_stops.read().await;
        assert_eq!(stops.get("ABC").map(|s| s.quantity), Some(Decimal::from(1)));
    }
}
