//! Reconciliation of locally Pending orders with the broker.

use super::Executor;
use crate::application::monitoring::connection_health_service::ConnectionStatus;
use crate::domain::trading::types::OrderStatus;
use anyhow::Result;
use std::sync::atomic::Ordering;
use tracing::{error, info, instrument, warn};

/// A pending order the broker does not know is only rejected after this age (ms),
/// since a just-submitted order may not be visible at the broker yet.
pub(super) const RECONCILE_GRACE_MS: i64 = 30_000;
/// First delay before retrying a failed reconciliation (doubled up to the max).
pub(super) const RECONCILE_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
pub(super) const RECONCILE_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);

impl Executor {
    pub(super) fn is_reconciled(&self) -> bool {
        self.reconciled.load(Ordering::SeqCst)
    }

    /// Runs a reconciliation pass and records whether it fully succeeded.
    pub(super) async fn try_reconcile(&self) -> bool {
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

    /// Exports reconciliation state for alerting (`rustrade_executor_reconciled`,
    /// `rustrade_orders_unknown_outcome`).
    pub(super) async fn publish_reconciliation_metrics(&self) {
        let metrics = self.agent_registry.metrics();
        metrics
            .executor_reconciled
            .set(if self.is_reconciled() { 1.0 } else { 0.0 });
        metrics
            .orders_unknown_outcome
            .set(self.unknown_orders.read().await.len() as f64);
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
    pub(super) async fn reconcile_pending(&self) -> Result<usize> {
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
}
