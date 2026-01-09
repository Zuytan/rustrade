use crate::domain::trading::types::Order;
use std::collections::HashMap;
use chrono::{DateTime, Duration, Utc};
use tracing::warn;

/// Tracks orders that have been sent but not yet confirmed/filled/cancelled
pub struct PendingOrdersTracker {
    /// Map of Client Order ID -> Order
    pending_orders: HashMap<String, Order>,
}

impl Default for PendingOrdersTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingOrdersTracker {
    pub fn new() -> Self {
        Self {
            pending_orders: HashMap::new(),
        }
    }

    /// Add an order to tracking
    pub fn add(&mut self, order: Order) {
        self.pending_orders.insert(order.id.clone(), order);
    }

    /// Remove an order from tracking (when filled/cancelled)
    pub fn remove(&mut self, order_id: &str) -> Option<Order> {
        self.pending_orders.remove(order_id)
    }

    /// Get total count
    pub fn count(&self) -> usize {
        self.pending_orders.len()
    }

    /// Check if we have pending orders for a specific symbol
    pub fn has_pending_for_symbol(&self, symbol: &str) -> bool {
        self.pending_orders.values().any(|o| o.symbol == symbol)
    }

    /// Calculate total pending exposure (approximate) for a symbol from BUY orders
    pub fn calculate_pending_exposure(&self, symbol: &str) -> rust_decimal::Decimal {
        self.pending_orders
            .values()
            .filter(|o| o.symbol == symbol && o.side == crate::domain::trading::types::OrderSide::Buy)
            .fold(rust_decimal::Decimal::ZERO, |acc, o| acc + (o.quantity * o.price))
    }

    /// Clean up stale pending orders (timeouts)
    /// Returns list of expired order IDs
    pub fn cleanup_stale_orders(&mut self, timeout_seconds: i64) -> Vec<String> {
        let now = Utc::now();
        let timeout = Duration::seconds(timeout_seconds);
        let mut expired = Vec::new();

        self.pending_orders.retain(|id, order| {
            // Verify timeout comparison with explicit types
            // assuming order.timestamp is in milliseconds based on usage (or update logic if seconds)
            // If order.timestamp is ms:
            let order_time = DateTime::<Utc>::from_timestamp(order.timestamp / 1000, (order.timestamp % 1000) as u32 * 1_000_000)
                .unwrap_or(Utc::now());
            
            let age = now.signed_duration_since(order_time);
            
            if age > timeout {
                warn!("Pending order {} timed out (stale)", id);
                expired.push(id.clone());
                false // Remove
            } else {
                true // Keep
            }
        });

        expired
    }
}
