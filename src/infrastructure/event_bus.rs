use crate::domain::events::{EventListener, TradingEvent};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Event bus for publishing trading events to multiple listeners
pub struct EventBus {
    listeners: Arc<RwLock<Vec<Arc<dyn EventListener>>>>,
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        Self {
            listeners: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Subscribe a listener to events
    pub async fn subscribe(&self, listener: Arc<dyn EventListener>) {
        self.listeners.write().await.push(listener);
    }

    /// Publish an event to all listeners
    pub async fn publish(&self, event: TradingEvent) {
        let listeners = self.listeners.read().await;
        for listener in listeners.iter() {
            listener.on_event(&event);
        }
    }

    /// Get count of subscribers (for testing)
    pub async fn subscriber_count(&self) -> usize {
        self.listeners.read().await.len()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            listeners: Arc::clone(&self.listeners),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::LoggingListener;
    use crate::domain::types::OrderSide;
    use rust_decimal_macros::dec;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingListener {
        count: Arc<AtomicUsize>,
    }

    impl EventListener for CountingListener {
        fn on_event(&self, _event: &TradingEvent) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn test_event_bus_subscribe() {
        let bus = EventBus::new();
        assert_eq!(bus.subscriber_count().await, 0);

        bus.subscribe(Arc::new(LoggingListener)).await;
        assert_eq!(bus.subscriber_count().await, 1);

        bus.subscribe(Arc::new(LoggingListener)).await;
        assert_eq!(bus.subscriber_count().await, 2);
    }

    #[tokio::test]
    async fn test_event_bus_publish() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));

        bus.subscribe(Arc::new(CountingListener {
            count: Arc::clone(&count),
        }))
        .await;

        let event = TradingEvent::SignalGenerated {
            symbol: "AAPL".to_string(),
            side: OrderSide::Buy,
            price: dec!(150.0),
            reason: "Test".to_string(),
            timestamp: 0,
        };

        bus.publish(event).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_event_bus_multiple_listeners() {
        let bus = EventBus::new();

        let count1 = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::new(AtomicUsize::new(0));

        bus.subscribe(Arc::new(CountingListener {
            count: Arc::clone(&count1),
        }))
        .await;
        bus.subscribe(Arc::new(CountingListener {
            count: Arc::clone(&count2),
        }))
        .await;

        let event = TradingEvent::TradeApproved {
            symbol: "NVDA".to_string(),
            side: OrderSide::Buy,
            quantity: dec!(5),
            reason: "Test".to_string(),
        };

        bus.publish(event).await;

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_event_bus_clone() {
        let bus1 = EventBus::new();
        let bus2 = bus1.clone();

        bus1.subscribe(Arc::new(LoggingListener)).await;

        // Clone should share the same listeners
        assert_eq!(bus2.subscriber_count().await, 1);
    }
}
