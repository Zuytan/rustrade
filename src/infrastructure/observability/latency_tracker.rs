use prometheus::Histogram;
use std::time::Instant;

/// RAII guard for measuring and recording latency
pub struct LatencyGuard {
    start: Instant,
    histogram: Histogram,
}

impl LatencyGuard {
    pub fn new(histogram: Histogram) -> Self {
        Self {
            start: Instant::now(),
            histogram,
        }
    }
}

impl Drop for LatencyGuard {
    fn drop(&mut self) {
        self.histogram.observe(self.start.elapsed().as_secs_f64());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::{Histogram, HistogramOpts};

    #[test]
    fn test_latency_guard_records_time() {
        let opts = HistogramOpts::new("test_latency", "test");
        let histogram = Histogram::with_opts(opts).unwrap();

        {
            let _guard = LatencyGuard::new(histogram.clone());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(histogram.get_sample_sum() >= 0.01);
        assert_eq!(histogram.get_sample_count(), 1);
    }
}
