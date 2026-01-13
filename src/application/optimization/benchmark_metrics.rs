use std::time::Instant;
use tracing::info;

/// Timer for measuring benchmark execution time
///
/// Automatically logs the elapsed time when dropped, making it easy to measure
/// code block execution time without manual cleanup.
///
/// # Example
///
/// ```
/// use rustrade::application::optimization::benchmark_metrics::BenchmarkTimer;
///
/// {
///     let _timer = BenchmarkTimer::new("My Operation");
///     // ... do work ...
/// } // Timer automatically logs elapsed time here
/// ```
pub struct BenchmarkTimer {
    start: Instant,
    label: String,
}

impl BenchmarkTimer {
    /// Create a new benchmark timer
    ///
    /// Logs a "Starting" message immediately and records the current time.
    ///
    /// # Arguments
    ///
    /// * `label` - Human-readable description of what is being timed
    pub fn new(label: &str) -> Self {
        info!("⏱️  Starting: {}", label);
        Self {
            start: Instant::now(),
            label: label.to_string(),
        }
    }

    /// Get elapsed time in seconds since timer creation
    pub fn elapsed_seconds(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Get elapsed time in milliseconds since timer creation
    pub fn elapsed_millis(&self) -> u128 {
        self.start.elapsed().as_millis()
    }
}

impl Drop for BenchmarkTimer {
    /// Automatically log the elapsed time when the timer goes out of scope
    fn drop(&mut self) {
        let elapsed = self.elapsed_seconds();
        info!("⏱️  Completed: {} in {:.2}s", self.label, elapsed);
    }
}

/// Statistics for a batch of benchmark runs
#[derive(Debug, Clone)]
pub struct BenchmarkStats {
    pub total_symbols: usize,
    pub successful: usize,
    pub failed: usize,
    pub total_time_seconds: f64,
    pub avg_time_per_symbol_seconds: f64,
    pub speedup_vs_sequential: Option<f64>,
}

impl BenchmarkStats {
    /// Create benchmark statistics from timing data
    ///
    /// # Arguments
    ///
    /// * `total_symbols` - Total number of symbols processed
    /// * `successful` - Number of successful backtests
    /// * `failed` - Number of failed backtests
    /// * `total_time_seconds` - Total wall-clock time
    /// * `sequential_time_seconds` - Optional sequential baseline for speedup calculation
    pub fn new(
        total_symbols: usize,
        successful: usize,
        failed: usize,
        total_time_seconds: f64,
        sequential_time_seconds: Option<f64>,
    ) -> Self {
        let avg_time_per_symbol_seconds = if total_symbols > 0 {
            total_time_seconds / total_symbols as f64
        } else {
            0.0
        };

        let speedup_vs_sequential = sequential_time_seconds.map(|seq_time| {
            if total_time_seconds > 0.0 {
                seq_time / total_time_seconds
            } else {
                0.0
            }
        });

        Self {
            total_symbols,
            successful,
            failed,
            total_time_seconds,
            avg_time_per_symbol_seconds,
            speedup_vs_sequential,
        }
    }

    /// Print a formatted summary of the benchmark statistics
    pub fn print_summary(&self) {
        println!("{}", "=".repeat(80));
        println!("📊 BENCHMARK STATISTICS");
        println!("{}", "=".repeat(80));
        println!("Total Symbols:        {}", self.total_symbols);
        println!("Successful:           {} ✅", self.successful);
        println!("Failed:               {} ❌", self.failed);
        println!("Total Time:           {:.2}s", self.total_time_seconds);
        println!(
            "Avg Time/Symbol:      {:.2}s",
            self.avg_time_per_symbol_seconds
        );

        if let Some(speedup) = self.speedup_vs_sequential {
            println!("Speedup vs Sequential: {:.2}x 🚀", speedup);
        }

        println!("{}", "=".repeat(80));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_benchmark_timer_elapsed() {
        let timer = BenchmarkTimer::new("test");
        thread::sleep(Duration::from_millis(100));

        let elapsed = timer.elapsed_seconds();
        assert!(elapsed >= 0.1, "Timer should measure at least 100ms");
        assert!(elapsed < 0.2, "Timer should not measure more than 200ms");
    }

    #[test]
    fn test_benchmark_timer_millis() {
        let timer = BenchmarkTimer::new("test");
        thread::sleep(Duration::from_millis(50));

        let elapsed_ms = timer.elapsed_millis();
        assert!(elapsed_ms >= 50, "Timer should measure at least 50ms");
    }

    #[test]
    fn test_benchmark_stats_creation() {
        let stats = BenchmarkStats::new(10, 8, 2, 120.0, Some(360.0));

        assert_eq!(stats.total_symbols, 10);
        assert_eq!(stats.successful, 8);
        assert_eq!(stats.failed, 2);
        assert_eq!(stats.total_time_seconds, 120.0);
        assert_eq!(stats.avg_time_per_symbol_seconds, 12.0);
        assert_eq!(stats.speedup_vs_sequential, Some(3.0));
    }

    #[test]
    fn test_benchmark_stats_no_speedup() {
        let stats = BenchmarkStats::new(5, 5, 0, 60.0, None);

        assert_eq!(stats.speedup_vs_sequential, None);
    }

    #[test]
    fn test_benchmark_stats_zero_symbols() {
        let stats = BenchmarkStats::new(0, 0, 0, 0.0, None);

        assert_eq!(stats.avg_time_per_symbol_seconds, 0.0);
    }
}
