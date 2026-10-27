//! Executor agent: submits approved orders, keeps the local portfolio and order
//! records in sync with the broker, and manages trailing stops.
//!
//! - `reconciliation`: pending-order reconciliation (fail-closed) and its metrics
//! - `acceptance`: post-submission path of accepted orders and portfolio updates
//! - `trailing_stops`: trailing stop updates on each candle

mod acceptance;
mod reconciliation;
#[cfg(test)]
mod tests;
mod trailing_stops;

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
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Order, OrderSide, OrderStatus};
use reconciliation::{RECONCILE_INITIAL_BACKOFF, RECONCILE_MAX_BACKOFF};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
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

    async fn check_timeouts(&self) {
        let actions = self.order_monitor.check_timeouts().await;
        for action in actions {
            match action {
                MonitorAction::None => {}
                _ => { /* Already handled in check_timeouts or similar? */ }
            }
        }
    }
}
