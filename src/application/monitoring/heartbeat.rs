use std::time::{Duration, Instant};
use tracing::debug;

/// Monitor for detecting silent (zombie) data streams.
pub struct StreamHealthMonitor {
    last_event_received_at: Instant,
    silence_threshold: Duration,
    name: String,
}

impl StreamHealthMonitor {
    pub fn new(name: &str, silence_threshold: Duration) -> Self {
        Self {
            last_event_received_at: Instant::now(),
            silence_threshold,
            name: name.to_string(),
        }
    }

    /// Record that an event has been received.
    pub fn record_event(&mut self) {
        self.last_event_received_at = Instant::now();
    }

    /// Check if the stream is still healthy.
    /// Returns true if healthy, false if the silence threshold has been exceeded.
    pub fn is_healthy(&self) -> bool {
        let elapsed = self.last_event_received_at.elapsed();
        if elapsed > self.silence_threshold {
            debug!(
                "StreamHealthMonitor[{}]: Stream is silent for {:?} (Threshold: {:?})",
                self.name, elapsed, self.silence_threshold
            );
            return false;
        }
        true
    }

    pub fn last_event_elapsed(&self) -> Duration {
        self.last_event_received_at.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_heartbeat_healthy() {
        let mut monitor = StreamHealthMonitor::new("test", Duration::from_secs(1));
        assert!(monitor.is_healthy());
        monitor.record_event();
        assert!(monitor.is_healthy());
    }

    #[test]
    fn test_heartbeat_unhealthy() {
        let monitor = StreamHealthMonitor::new("test", Duration::from_millis(10));
        thread::sleep(Duration::from_millis(20));
        assert!(!monitor.is_healthy());
    }
}
