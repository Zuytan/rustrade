use crate::domain::trading::types::Order;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time;
use tracing::{info, warn};

pub struct OrderThrottler {
    order_rx: Receiver<Order>,
    throttled_order_tx: Sender<Order>,
    max_orders_per_minute: u32,
    window_duration: Duration, // Configurable for testing
    recent_orders: VecDeque<Instant>,
    queued_orders: VecDeque<Order>,
}

impl OrderThrottler {
    pub fn new(
        order_rx: Receiver<Order>,
        throttled_order_tx: Sender<Order>,
        max_orders_per_minute: u32,
    ) -> Self {
        Self::with_window(
            order_rx,
            throttled_order_tx,
            max_orders_per_minute,
            Duration::from_secs(60),
        )
    }

    pub fn with_window(
        order_rx: Receiver<Order>,
        throttled_order_tx: Sender<Order>,
        max_orders_per_minute: u32,
        window_duration: Duration,
    ) -> Self {
        Self {
            order_rx,
            throttled_order_tx,
            max_orders_per_minute,
            window_duration,
            recent_orders: VecDeque::new(),
            queued_orders: VecDeque::new(),
        }
    }

    pub async fn run(&mut self) {
        info!(
            "OrderThrottler started (limit: {} orders/min)",
            self.max_orders_per_minute
        );

        let mut tick_interval = time::interval(Duration::from_millis(100));
        tick_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                biased; // Process in order: tick first, then new orders

                // Periodic tick to process queued orders
                _ = tick_interval.tick() => {
                    self.process_queue().await;
                }

                // Receive new orders from RiskManager
                Some(order) = self.order_rx.recv() => {
                    self.handle_incoming_order(order).await;
                    // Try to process queue immediately if we have capacity
                    self.process_queue().await;
                }
            }
        }
    }

    async fn handle_incoming_order(&mut self, order: Order) {
        self.cleanup_old_timestamps();

        if self.can_accept_order() {
            // Accept immediately
            self.forward_order(order).await;
        } else {
            // Throttle: add to queue (FIFO)
            warn!(
                "OrderThrottler: Rate limit reached. Queueing order {}. Queue size: {}",
                order.id,
                self.queued_orders.len() + 1
            );
            self.queued_orders.push_back(order); // FIFO: push_back
        }
    }

    async fn process_queue(&mut self) {
        self.cleanup_old_timestamps();

        while !self.queued_orders.is_empty() && self.can_accept_order() {
            if let Some(order) = self.queued_orders.pop_front() {
                // FIFO: pop_front
                info!(
                    "OrderThrottler: Processing queued order {}. Remaining: {}",
                    order.id,
                    self.queued_orders.len()
                );
                self.forward_order(order).await;
            }
        }
    }

    async fn forward_order(&mut self, order: Order) {
        self.recent_orders.push_back(Instant::now());

        if let Err(e) = self.throttled_order_tx.send(order.clone()).await {
            tracing::error!(
                "OrderThrottler: Failed to forward order {}: {}",
                order.id,
                e
            );
        } else {
            info!("OrderThrottler: Forwarded order {}", order.id);
        }
    }

    fn can_accept_order(&self) -> bool {
        (self.recent_orders.len() as u32) < self.max_orders_per_minute
    }

    fn cleanup_old_timestamps(&mut self) {
        let window_ago = Instant::now() - self.window_duration;

        while let Some(&timestamp) = self.recent_orders.front() {
            if timestamp < window_ago {
                self.recent_orders.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::{OrderSide, OrderStatus, OrderType};
    use chrono::Utc;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use tokio::sync::mpsc;

    fn create_test_order(id: &str) -> Order {
        Order {
            id: id.to_string(),
            symbol: "BTC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: dec!(1.0),
            order_type: OrderType::Market,
            status: OrderStatus::New,
            timestamp: Utc::now().timestamp_millis(),
        }
    }

    #[tokio::test]
    async fn test_accepts_order_under_limit() {
        let (order_tx, order_rx) = mpsc::channel(10);
        let (throttled_tx, mut throttled_rx) = mpsc::channel(10);

        let mut throttler =
            OrderThrottler::with_window(order_rx, throttled_tx, 5, Duration::from_secs(1));

        tokio::spawn(async move {
            throttler.run().await;
        });

        // Send 3 orders (under limit of 5)
        for i in 1..=3 {
            order_tx
                .send(create_test_order(&format!("order-{}", i)))
                .await
                .unwrap();
        }

        // All should be forwarded immediately
        for _ in 1..=3 {
            assert!(throttled_rx.recv().await.is_some());
        }
    }

    #[tokio::test]
    async fn test_queues_order_over_limit() {
        let (order_tx, order_rx) = mpsc::channel(10);
        let (throttled_tx, mut throttled_rx) = mpsc::channel(10);

        let mut throttler =
            OrderThrottler::with_window(order_rx, throttled_tx, 2, Duration::from_secs(1));

        tokio::spawn(async move {
            throttler.run().await;
        });

        // Send 4 orders (over limit of 2)
        for i in 1..=4 {
            order_tx
                .send(create_test_order(&format!("order-{}", i)))
                .await
                .unwrap();
        }

        // First 2 should go through immediately
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(throttled_rx.try_recv().is_ok());
        assert!(throttled_rx.try_recv().is_ok());

        // Next 2 should be queued (not immediately available)
        assert!(throttled_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_sliding_window_cleanup() {
        let (order_tx, order_rx) = mpsc::channel(10);
        let (throttled_tx, mut throttled_rx) = mpsc::channel(10);

        let mut throttler =
            OrderThrottler::with_window(order_rx, throttled_tx, 2, Duration::from_secs(1));

        tokio::spawn(async move {
            throttler.run().await;
        });

        // Send 2 orders
        order_tx.send(create_test_order("order-1")).await.unwrap();
        order_tx.send(create_test_order("order-2")).await.unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(throttled_rx.try_recv().is_ok());
        assert!(throttled_rx.try_recv().is_ok());
    }

    // NEW TEST: Verify FIFO order preservation
    #[tokio::test]
    async fn test_fifo_order_preservation() {
        let (order_tx, order_rx) = mpsc::channel(20);
        let (throttled_tx, mut throttled_rx) = mpsc::channel(20);

        let mut throttler = OrderThrottler::with_window(
            order_rx,
            throttled_tx,
            3,
            Duration::from_secs(1), // 1-second window for testing
        );

        tokio::spawn(async move {
            throttler.run().await;
        });

        // Send 10 orders rapidly
        let order_ids: Vec<String> = (1..=10).map(|i| format!("order-{}", i)).collect();

        for id in &order_ids {
            order_tx.send(create_test_order(id)).await.unwrap();
        }

        // Collect all forwarded orders
        let mut received_ids = Vec::new();
        for _ in 0..10 {
            if let Some(order) = tokio::time::timeout(Duration::from_secs(2), throttled_rx.recv())
                .await
                .ok()
                .flatten()
            {
                received_ids.push(order.id);
            }
        }

        // Verify FIFO: received order should match sent order
        assert_eq!(received_ids.len(), 10, "Should receive all 10 orders");
        assert_eq!(received_ids, order_ids, "Order should be preserved (FIFO)");
    }

    // NEW TEST: Verify no orders are lost during throttling
    #[tokio::test]
    async fn test_no_order_loss() {
        let (order_tx, order_rx) = mpsc::channel(100);
        let (throttled_tx, mut throttled_rx) = mpsc::channel(100);

        let mut throttler =
            OrderThrottler::with_window(order_rx, throttled_tx, 5, Duration::from_secs(1));

        tokio::spawn(async move {
            throttler.run().await;
        });

        // Send 20 orders
        for i in 1..=20 {
            order_tx
                .send(create_test_order(&format!("order-{}", i)))
                .await
                .unwrap();
        }

        // Wait and collect all orders
        let mut received_count = 0;
        let timeout_duration = Duration::from_secs(5);
        let start = Instant::now();

        while received_count < 20 && start.elapsed() < timeout_duration {
            if let Ok(Some(_)) =
                tokio::time::timeout(Duration::from_millis(100), throttled_rx.recv()).await
            {
                received_count += 1;
            }
        }

        assert_eq!(
            received_count, 20,
            "All 20 orders should be received, none lost"
        );
    }

    // NEW TEST: Verify rate limiting actually works (guards Executor)
    #[tokio::test]
    async fn test_rate_limiting_guards_executor() {
        let (order_tx, order_rx) = mpsc::channel(100);
        let (throttled_tx, mut throttled_rx) = mpsc::channel(100);

        let limit = 5;
        let mut throttler = OrderThrottler::with_window(
            order_rx,
            throttled_tx,
            limit,
            Duration::from_secs(1), // 1-second window for testing
        );

        tokio::spawn(async move {
            throttler.run().await;
        });

        // Send 10 orders instantly
        for i in 1..=10 {
            order_tx
                .send(create_test_order(&format!("order-{}", i)))
                .await
                .unwrap();
        }

        // Check how many arrive in first 200ms (should be <= limit)
        tokio::time::sleep(Duration::from_millis(200)).await;

        let mut immediate_count = 0;
        while throttled_rx.try_recv().is_ok() {
            immediate_count += 1;
        }

        assert!(
            immediate_count <= limit as usize,
            "Only {} orders should pass immediately, got {}",
            limit,
            immediate_count
        );

        // Wait for remaining orders to process
        let mut total_count = immediate_count;
        while total_count < 10 {
            if let Ok(Some(_)) =
                tokio::time::timeout(Duration::from_secs(2), throttled_rx.recv()).await
            {
                total_count += 1;
            } else {
                break;
            }
        }

        assert_eq!(total_count, 10, "Eventually all orders should be processed");
    }
}
