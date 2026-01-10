//! Push-based observability for Rustrade
//!
//! This module provides observability through **outbound data only** - no HTTP server,
//! no incoming requests. Metrics are pushed via:
//!
//! 1. **Structured JSON Logs**: Periodic JSON output to stdout (for Loki, Fluentd, CloudWatch)
//! 2. **Prometheus Pushgateway** (optional): For integration with Prometheus
//!
//! **Security**: This system only SENDS data, it never accepts requests.

pub mod metrics;
pub mod reporter;

pub use metrics::Metrics;
pub use reporter::MetricsReporter;
