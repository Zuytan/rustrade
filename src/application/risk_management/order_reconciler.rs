use crate::application::monitoring::portfolio_state_manager::{
    PortfolioStateManager, ReservationToken,
};
use crate::domain::ports::OrderUpdate;
use crate::domain::risk::state::RiskState;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{OrderSide, OrderStatus};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
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

    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    pub fn handle_order_update(
        &mut self,
        update: &OrderUpdate,
        risk_state: &mut RiskState,
        portfolio_state_manager: &Arc<PortfolioStateManager>,
    ) -> bool {
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
                OrderStatus::Cancelled
                | OrderStatus::Rejected
                | OrderStatus::Expired
                | OrderStatus::Suspended => {
                    // Terminal states handled below
                }
                _ => {}
            }

            // Cleanup only non-fill terminal states
            if matches!(
                update.status,
                OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired
            ) {
                self.remove_order(&update.client_order_id, portfolio_state_manager);
            }
        }

        state_changed
    }

    /// Cleanup tentative filled orders and release reservations
    pub fn reconcile_pending_orders(
        &mut self,
        portfolio: &Portfolio,
        portfolio_state_manager: &Arc<PortfolioStateManager>,
    ) {
        let ttl_ms = self.ttl_ms;

        // Identify orders to remove first to avoid borrowing issues
        let mut to_remove = Vec::new();

        for (order_id, pending) in &self.pending_orders {
            if pending.filled_but_not_synced {
                // Check TTL
                if let Some(filled_at) = pending.filled_at {
                    let age_ms = chrono::Utc::now().timestamp_millis() - filled_at;
                    if age_ms > ttl_ms {
                        warn!(
                            "RiskManager: Pending order {} TTL expired after {}ms. Forcing cleanup for {}",
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
            }
        }

        for order_id in to_remove {
            self.remove_order(&order_id, portfolio_state_manager);
        }
    }

    pub fn remove_order(
        &mut self,
        order_id: &str,
        portfolio_state_manager: &Arc<PortfolioStateManager>,
    ) {
        self.pending_orders.remove(order_id);

        if let Some(token) = self.pending_reservations.remove(order_id) {
            let mgr = portfolio_state_manager.clone();
            tokio::spawn(async move {
                mgr.release_reservation(token).await;
            });
        }
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
