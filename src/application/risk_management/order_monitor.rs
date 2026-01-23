use crate::application::risk_management::order_retry_strategy::RetryConfig;
use crate::domain::trading::types::{Order, OrderType};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MonitoredOrder {
    pub order: Order,
    pub created_at: i64,
}

pub struct OrderMonitor {
    active_orders: Arc<RwLock<HashMap<String, MonitoredOrder>>>,
    retry_config: RetryConfig,
}

pub enum MonitorAction {
    None,
    CancelAndReplace {
        order_id_to_cancel: String,
        original_order: Box<Order>,
        new_market_order: Box<Order>,
    },
}

impl OrderMonitor {
    pub fn new(retry_config: RetryConfig) -> Self {
        Self {
            active_orders: Arc::new(RwLock::new(HashMap::new())),
            retry_config,
        }
    }

    pub async fn track_order(&self, order: Order) {
        if order.order_type == OrderType::Limit && self.retry_config.enable_retry {
            let mut orders = self.active_orders.write().await;
            orders.insert(
                order.id.clone(),
                MonitoredOrder {
                    order: order.clone(),
                    created_at: chrono::Utc::now().timestamp_millis(),
                },
            );
            info!("OrderMonitor: Tracking Limit Order {}", order.id);
        }
    }

    pub async fn on_order_filled(&self, order_id: &str) {
        let mut orders = self.active_orders.write().await;
        if orders.remove(order_id).is_some() {
            info!("OrderMonitor: Order {} filled, stopped tracking", order_id);
        }
    }

    pub async fn on_order_canceled(&self, order_id: &str) {
        let mut orders = self.active_orders.write().await;
        if orders.remove(order_id).is_some() {
            info!(
                "OrderMonitor: Order {} canceled, stopped tracking",
                order_id
            );
        }
    }

    pub async fn check_timeouts(&self) -> Vec<MonitorAction> {
        if !self.retry_config.enable_retry {
            return vec![];
        }

        let now = chrono::Utc::now().timestamp_millis();
        let timeout = self.retry_config.limit_timeout_ms as i64;
        let mut actions = Vec::new();
        let mut timed_out_ids = Vec::new();

        let orders = self.active_orders.read().await;
        for (id, monitored) in orders.iter() {
            if now - monitored.created_at > timeout {
                warn!(
                    "OrderMonitor: Limit Order {} timed out ({}ms > {}ms). Triggering replacement.",
                    id,
                    now - monitored.created_at,
                    timeout
                );

                // Create replacement Market Order
                let market_order = Order {
                    id: Uuid::new_v4().to_string(),
                    symbol: monitored.order.symbol.clone(),
                    side: monitored.order.side,
                    price: Decimal::ZERO, // Market order
                    quantity: monitored.order.quantity,
                    order_type: OrderType::Market,
                    timestamp: now,
                };

                actions.push(MonitorAction::CancelAndReplace {
                    order_id_to_cancel: id.clone(),
                    original_order: Box::new(monitored.order.clone()),
                    new_market_order: Box::new(market_order),
                });

                timed_out_ids.push(id.clone());
            }
        }
        drop(orders); // Release read lock

        // Remove timed out orders from tracking to avoid double replacement
        if !timed_out_ids.is_empty() {
            let mut orders = self.active_orders.write().await;
            for id in timed_out_ids {
                orders.remove(&id);
            }
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::{OrderSide, OrderType};
    use rust_decimal_macros::dec;

    fn create_limit_order(id: &str, price: Decimal) -> Order {
        Order {
            id: id.to_string(),
            symbol: "BTC/USD".to_string(),
            side: OrderSide::Buy,
            price,
            quantity: dec!(1.0),
            order_type: OrderType::Limit,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    #[tokio::test]
    async fn test_track_and_timeout_order() {
        let config = RetryConfig {
            limit_timeout_ms: 100, // Short timeout for testing
            enable_retry: true,
        };
        let monitor = OrderMonitor::new(config);

        // 1. Track order
        let order = create_limit_order("ord_1", dec!(50000));
        monitor.track_order(order.clone()).await;

        // Should be tracked (active count verified by check_timeouts finding nothing yet)
        let actions = monitor.check_timeouts().await;
        assert!(actions.is_empty(), "Order should not timeout immediately");

        // 2. Wait for timeout
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // 3. Check timeouts
        let actions = monitor.check_timeouts().await;
        assert_eq!(actions.len(), 1, "Order should have timed out");

        match &actions[0] {
            MonitorAction::CancelAndReplace {
                order_id_to_cancel,
                new_market_order,
                original_order,
            } => {
                assert_eq!(order_id_to_cancel, "ord_1");
                assert_eq!(new_market_order.order_type, OrderType::Market);
                assert_eq!(new_market_order.symbol, "BTC/USD");
                assert_eq!(new_market_order.quantity, dec!(1.0));
                assert_eq!(new_market_order.price, Decimal::ZERO); // Market order has 0 price usually or implied
                assert_eq!(original_order.id, "ord_1");
            }
            _ => panic!("Expected CancelAndReplace action"),
        }

        // 4. Verify order is removed from tracking effectively (check_timeouts won't return it again if logic handles it?)
        // The current implementation of check_timeouts REMOVES the order when returning action.
        let actions_retry = monitor.check_timeouts().await;
        assert!(
            actions_retry.is_empty(),
            "Order should be removed after timeout action triggered"
        );
    }

    #[tokio::test]
    async fn test_order_fill_removes_from_monitor() {
        let config = RetryConfig {
            limit_timeout_ms: 5000,
            enable_retry: true,
        };
        let monitor = OrderMonitor::new(config);

        let order = create_limit_order("ord_2", dec!(50000));
        monitor.track_order(order.clone()).await;

        // Simulate Fill
        monitor.on_order_filled("ord_2").await;

        // Should not timeout
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await; // Minor delay
        let actions = monitor.check_timeouts().await;
        assert!(actions.is_empty(), "Filled order should not timeout");
    }

    #[tokio::test]
    async fn test_order_cancel_removes_from_monitor() {
        let config = RetryConfig {
            limit_timeout_ms: 5000,
            enable_retry: true,
        };
        let monitor = OrderMonitor::new(config);

        let order = create_limit_order("ord_3", dec!(50000));
        monitor.track_order(order.clone()).await;

        // Simulate Cancel
        monitor.on_order_canceled("ord_3").await;

        let actions = monitor.check_timeouts().await;
        assert!(actions.is_empty(), "Canceled order should not timeout");
    }

    #[tokio::test]
    async fn test_disabled_retry() {
        let config = RetryConfig {
            limit_timeout_ms: 100,
            enable_retry: false, // Disabled
        };
        let monitor = OrderMonitor::new(config);

        let order = create_limit_order("ord_4", dec!(50000));
        monitor.track_order(order.clone()).await;

        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        let actions = monitor.check_timeouts().await;
        assert!(
            actions.is_empty(),
            "Should not generate actions if retry is disabled"
        );
    }
}
