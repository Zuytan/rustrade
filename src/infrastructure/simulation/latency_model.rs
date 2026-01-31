use rand::Rng;
use std::time::Duration;

/// Trait defining a network latency simulation model.
pub trait LatencyModel: Send + Sync {
    /// Returns the duration to wait before confirming the order.
    fn next_latency(&self) -> Duration;
}

/// A simple latency model with a base latency and random jitter.
/// Simulates network RTT (Round Trip Time) + Exchange Processing Time.
#[derive(Debug, Clone)]
pub struct NetworkLatency {
    base_ms: u64,
    jitter_ms: u64,
}

impl NetworkLatency {
    pub fn new(base_ms: u64, jitter_ms: u64) -> Self {
        Self { base_ms, jitter_ms }
    }
}

impl LatencyModel for NetworkLatency {
    fn next_latency(&self) -> Duration {
        let mut rng = rand::rng();
        // Jitter between -jitter_ms and +jitter_ms
        let jitter = rng.random_range(-(self.jitter_ms as i64)..=(self.jitter_ms as i64));

        let ms = (self.base_ms as i64 + jitter).max(0) as u64;
        Duration::from_millis(ms)
    }
}

/// Zero latency model (instant execution) for tests or pure logic verification.
pub struct ZeroLatency;

impl LatencyModel for ZeroLatency {
    fn next_latency(&self) -> Duration {
        Duration::from_millis(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_latency_range() {
        let model = NetworkLatency::new(50, 10);
        for _ in 0..100 {
            let lat = model.next_latency().as_millis() as u64;
            assert!(
                lat >= 40 && lat <= 60,
                "Latency {} out of bounds [40, 60]",
                lat
            );
        }
    }
}
