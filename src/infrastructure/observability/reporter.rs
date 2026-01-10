//! Push-based metrics reporter for Rustrade
//!
//! Periodically outputs metrics as structured JSON to stdout.
//! Can optionally push to Prometheus Pushgateway.
//!
//! **Security**: This system only SENDS data, never accepts requests.

use crate::domain::trading::portfolio::Portfolio;
use crate::infrastructure::observability::metrics::Metrics;
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Metrics snapshot for JSON output
#[derive(Serialize)]
pub struct MetricsSnapshot {
    pub timestamp: String,
    pub uptime_seconds: u64,
    pub version: String,
    pub portfolio: PortfolioSnapshot,
    pub system: SystemSnapshot,
}

#[derive(Serialize)]
pub struct PortfolioSnapshot {
    pub cash_usd: f64,
    pub total_value_usd: f64,
    pub positions_count: usize,
    pub positions: Vec<PositionSnapshot>,
}

#[derive(Serialize)]
pub struct PositionSnapshot {
    pub symbol: String,
    pub quantity: f64,
    pub average_price: f64,
    pub current_value: f64,
}

#[derive(Serialize)]
pub struct SystemSnapshot {
    pub circuit_breaker_tripped: bool,
    pub sentiment_score: Option<u32>,
}

/// Push-based metrics reporter
///
/// Outputs metrics as structured JSON logs on a configurable interval.
/// No HTTP server, no incoming connections - only outbound data.
pub struct MetricsReporter {
    portfolio: Arc<RwLock<Portfolio>>,
    metrics: Metrics,
    start_time: Instant,
    interval: Duration,
}

impl MetricsReporter {
    /// Create a new metrics reporter
    ///
    /// # Arguments
    /// * `portfolio` - Shared portfolio state
    /// * `metrics` - Prometheus metrics (for internal tracking)
    /// * `interval_seconds` - How often to output metrics (default: 60)
    pub fn new(portfolio: Arc<RwLock<Portfolio>>, metrics: Metrics, interval_seconds: u64) -> Self {
        Self {
            portfolio,
            metrics,
            start_time: Instant::now(),
            interval: Duration::from_secs(interval_seconds),
        }
    }

    /// Run the reporter in a loop, outputting metrics periodically
    pub async fn run(self) {
        info!(
            "MetricsReporter: Starting push-based metrics (interval: {:?})",
            self.interval
        );
        info!("MetricsReporter: Metrics will be output as JSON to stdout");

        loop {
            tokio::time::sleep(self.interval).await;

            match self.collect_snapshot().await {
                Ok(snapshot) => {
                    // Output as structured JSON log
                    match serde_json::to_string(&snapshot) {
                        Ok(json) => {
                            // Use a special prefix so logs can be easily filtered
                            println!("METRICS_JSON:{}", json);
                            info!(
                                "Portfolio: ${:.2} | Positions: {} | Uptime: {}s",
                                snapshot.portfolio.total_value_usd,
                                snapshot.portfolio.positions_count,
                                snapshot.uptime_seconds
                            );
                        }
                        Err(e) => warn!("Failed to serialize metrics: {}", e),
                    }
                }
                Err(e) => warn!("Failed to collect metrics: {}", e),
            }
        }
    }

    /// Collect current metrics snapshot
    async fn collect_snapshot(&self) -> anyhow::Result<MetricsSnapshot> {
        let portfolio = self.portfolio.read().await;
        let uptime = self.start_time.elapsed().as_secs();

        // Calculate portfolio value
        let cash = portfolio.cash.to_f64().unwrap_or(0.0);
        let positions_value: f64 = portfolio
            .positions
            .values()
            .map(|p| p.quantity.to_f64().unwrap_or(0.0) * p.average_price.to_f64().unwrap_or(0.0))
            .sum();

        let positions: Vec<PositionSnapshot> = portfolio
            .positions
            .iter()
            .map(|(symbol, pos)| {
                let quantity = pos.quantity.to_f64().unwrap_or(0.0);
                let average_price = pos.average_price.to_f64().unwrap_or(0.0);
                PositionSnapshot {
                    symbol: symbol.clone(),
                    quantity,
                    average_price,
                    current_value: quantity * average_price,
                }
            })
            .collect();

        // Update internal metrics
        self.metrics.portfolio_cash_usd.set(cash);
        self.metrics.portfolio_value_usd.set(cash + positions_value);
        self.metrics
            .positions_count
            .set(portfolio.positions.len() as f64);
        self.metrics.uptime_seconds.set(uptime as f64);

        Ok(MetricsSnapshot {
            timestamp: chrono::Utc::now().to_rfc3339(),
            uptime_seconds: uptime,
            version: env!("CARGO_PKG_VERSION").to_string(),
            portfolio: PortfolioSnapshot {
                cash_usd: cash,
                total_value_usd: cash + positions_value,
                positions_count: portfolio.positions.len(),
                positions,
            },
            system: SystemSnapshot {
                circuit_breaker_tripped: false, // TODO: Wire up from RiskManager state
                sentiment_score: None,          // TODO: Wire up from sentiment provider
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::portfolio::Portfolio;

    #[tokio::test]
    async fn test_metrics_snapshot_collection() {
        let portfolio = Arc::new(RwLock::new(Portfolio::new()));
        let metrics = Metrics::new().expect("Failed to create metrics");
        let reporter = MetricsReporter::new(portfolio, metrics, 60);

        let snapshot = reporter
            .collect_snapshot()
            .await
            .expect("Failed to collect snapshot");

        assert_eq!(snapshot.portfolio.positions_count, 0);
        assert!(!snapshot.timestamp.is_empty());
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = MetricsSnapshot {
            timestamp: "2026-01-10T10:00:00Z".to_string(),
            uptime_seconds: 3600,
            version: "0.62.0".to_string(),
            portfolio: PortfolioSnapshot {
                cash_usd: 50000.0,
                total_value_usd: 75000.0,
                positions_count: 2,
                positions: vec![PositionSnapshot {
                    symbol: "AAPL".to_string(),
                    quantity: 100.0,
                    average_price: 150.0,
                    current_value: 15000.0,
                }],
            },
            system: SystemSnapshot {
                circuit_breaker_tripped: false,
                sentiment_score: Some(50),
            },
        };

        let json = serde_json::to_string(&snapshot).expect("Failed to serialize");
        assert!(json.contains("AAPL"));
        assert!(json.contains("50000"));
    }
}
