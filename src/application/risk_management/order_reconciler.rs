use crate::application::monitoring::portfolio_state_manager::ReservationToken;
use crate::domain::ports::OrderUpdate;
use crate::domain::risk::state::RiskState;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{OrderSide, OrderStatus};
use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct PendingOrder {
    pub symbol: String,
    pub side: OrderSide,
    pub requested_qty: Decimal,
    pub filled_qty: Decimal,
    pub filled_but_not_synced: bool, // Track filled orders awaiting portfolio confirmation
    pub entry_price: Decimal,        // Track for P&L calculation on sell
    pub filled_at: Option<i64>,      // Timestamp when filled (for TTL cleanup)
    pub submitted_at: i64,           // Timestamp when order was submitted (for stale pending TTL)
}

pub struct OrderReconciler {
    pub pending_orders: HashMap<String, PendingOrder>,
    pub pending_reservations: HashMap<String, ReservationToken>,
    ttl_ms: i64,
}

impl OrderReconciler {
    pub fn new(ttl_ms: Option<i64>) -> Self {
        Self {
            pending_orders: HashMap::new(),
            pending_reservations: HashMap::new(),
            ttl_ms: ttl_ms.unwrap_or(300_000), // Default 5 mins
        }
    }

    pub fn track_order(&mut self, order_id: String, order: PendingOrder) {
        self.pending_orders.insert(order_id, order);
    }

    pub fn add_reservation(&mut self, order_id: String, token: ReservationToken) {
        self.pending_reservations.insert(order_id, token);
    }

    /// Handle real-time order updates to maintain pending state.
    ///
    /// Returns `(state_changed, Option<ReservationToken>)`.
    /// - `state_changed`: true if risk state (e.g. consecutive losses) changed and needs persistence.
    /// - `Option<ReservationToken>`: token to release if order reached a terminal state.
    pub fn handle_order_update(
        &mut self,
        update: &OrderUpdate,
        risk_state: &mut RiskState,
    ) -> (bool, Option<ReservationToken>) {
        let mut state_changed = false;

        if let Some(pending) = self.pending_orders.get_mut(&update.client_order_id) {
            match update.status {
                OrderStatus::Filled | OrderStatus::PartiallyFilled => {
                    pending.filled_qty = update.filled_qty;
                    if pending.filled_qty >= pending.requested_qty {
                        // Full fill: Mark as tentative instead of removing
                        pending.filled_but_not_synced = true;
                        pending.filled_at = Some(chrono::Utc::now().timestamp_millis());

                        // Track P&L for SELL orders to update consecutive loss counter
                        if pending.side == OrderSide::Sell
                            && let Some(fill_price) = update.filled_avg_price
                        {
                            let pnl = (fill_price - pending.entry_price) * pending.filled_qty;
                            if pnl < Decimal::ZERO {
                                risk_state.consecutive_losses += 1;
                                warn!(
                                    "RiskManager: Trade LOSS detected for {} (${:.2}). Consecutive losses: {}",
                                    pending.symbol, pnl, risk_state.consecutive_losses
                                );
                                state_changed = true;
                            } else {
                                risk_state.consecutive_losses = 0;
                                state_changed = true;
                                info!(
                                    "RiskManager: Trade PROFIT for {} (${:.2}). Loss streak reset.",
                                    pending.symbol, pnl
                                );
                            }
                        }

                        info!(
                            "RiskManager: Order {} FILLED (tentative) - awaiting portfolio sync for {}",
                            &update.client_order_id[..8],
                            pending.symbol
                        );
                    }
                }
                OrderStatus::Canceled
                | OrderStatus::Rejected
                | OrderStatus::Expired
                | OrderStatus::Suspended => {
                    // Terminal states handled below
                }
                _ => {}
            }

            // Cleanup only non-fill terminal states — return token for caller to release
            if matches!(
                update.status,
                OrderStatus::Canceled | OrderStatus::Rejected | OrderStatus::Expired
            ) {
                let token = self.remove_order(&update.client_order_id);
                return (state_changed, token);
            }
        }

        (state_changed, None)
    }

    /// Cleanup stale pending orders and tentative filled orders.
    ///
    /// Returns `ReservationToken`s that must be released by the caller (async context).
    /// This avoids `tokio::spawn` which can cause timing issues in tests and provides
    /// deterministic reservation release ordering.
    pub fn reconcile_pending_orders(&mut self, portfolio: &Portfolio) -> Vec<ReservationToken> {
        let ttl_ms = self.ttl_ms;
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Identify orders to remove first to avoid borrowing issues
        let mut to_remove = Vec::new();

        for (order_id, pending) in &self.pending_orders {
            if pending.filled_but_not_synced {
                // --- Filled-but-not-synced path ---
                // Check TTL since fill
                if let Some(filled_at) = pending.filled_at {
                    let age_ms = now_ms - filled_at;
                    if age_ms > ttl_ms {
                        warn!(
                            "RiskManager: Filled order {} TTL expired after {}ms. Forcing cleanup for {}",
                            &order_id[..8],
                            age_ms,
                            pending.symbol
                        );
                        to_remove.push(order_id.clone());
                        continue;
                    }
                }

                // Check if position exists in portfolio
                let normalized_symbol = pending.symbol.replace("/", "").replace(" ", "");
                let in_portfolio = portfolio.positions.iter().any(|(sym, pos)| {
                    let normalized_sym = sym.replace("/", "").replace(" ", "");
                    normalized_sym == normalized_symbol && pos.quantity > Decimal::ZERO
                });

                if in_portfolio {
                    info!(
                        "RiskManager: Reconciled order {} - {} now confirmed in portfolio",
                        &order_id[..8],
                        pending.symbol
                    );
                    to_remove.push(order_id.clone());
                }
            } else {
                // --- Still-pending path (never filled) ---
                // If order has been pending for longer than TTL, it's stale — clean up
                let age_ms = now_ms - pending.submitted_at;
                if age_ms > ttl_ms {
                    warn!(
                        "RiskManager: Stale pending order {} expired after {}ms (never filled). Cleaning up for {}",
                        &order_id[..8],
                        age_ms,
                        pending.symbol
                    );
                    to_remove.push(order_id.clone());
                }
            }
        }

        let mut released_tokens = Vec::new();
        for order_id in to_remove {
            let token = self.remove_order_internal(&order_id);
            if let Some(t) = token {
                released_tokens.push(t);
            }
        }
        released_tokens
    }

    /// Remove a pending order and return its reservation token (if any) for the caller to release.
    pub fn remove_order(&mut self, order_id: &str) -> Option<ReservationToken> {
        self.remove_order_internal(order_id)
    }

    /// Internal: remove order and extract reservation token without releasing it.
    fn remove_order_internal(&mut self, order_id: &str) -> Option<ReservationToken> {
        self.pending_orders.remove(order_id);
        self.pending_reservations.remove(order_id)
    }

    pub fn get_pending_exposure(&self, symbol: &str, side: OrderSide) -> Decimal {
        self.pending_orders
            .values()
            .filter(|p| p.symbol == symbol && p.side == side)
            .fold(Decimal::ZERO, |acc, p| {
                acc + (p.requested_qty * p.entry_price)
            })
    }
}
