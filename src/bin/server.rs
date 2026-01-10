//! Rustrade Server - Headless trading system
//!
//! This binary runs the trading system without a GUI, suitable for
//! server deployments. Metrics are pushed via structured JSON logs
//! to stdout - no HTTP server, no incoming connections.
//!
//! # Usage
//! ```sh
//! OBSERVABILITY_INTERVAL=60 cargo run --bin server
//! ```
//!
//! # Environment Variables
//! - `OBSERVABILITY_ENABLED` - Enable metrics reporting (default: true)
//! - `OBSERVABILITY_INTERVAL` - Interval in seconds between metric outputs (default: 60)
//!
//! # Metrics Output
//! Metrics are output as JSON to stdout with prefix `METRICS_JSON:`.
//! Example: `METRICS_JSON:{"timestamp":"...", "portfolio":{...}}`
//!
//! This can be collected by:
//! - Log aggregators (Loki, Fluentd, CloudWatch Logs)
//! - File-based collection (redirect stdout to file)
//! - Prometheus Pushgateway (future enhancement)

use anyhow::Result;
use rustrade::application::system::Application;
use rustrade::config::Config;
use rustrade::infrastructure::observability::{Metrics, MetricsReporter};
use tracing::{Level, info};
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Setup logging (stdout only, no UI channel needed)
    let stdout_layer = tracing_subscriber::fmt::layer().with_target(false).pretty();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .with(stdout_layer)
        .init();

    info!("Rustrade Server {} starting...", env!("CARGO_PKG_VERSION"));
    info!("Mode: HEADLESS (no UI, no HTTP server)");
    info!("Metrics: Push-based (JSON to stdout)");

    // Load configuration
    let config = Config::from_env()?;
    info!(
        "Configuration loaded: Mode={:?}, Asset={:?}, Symbols={:?}",
        config.mode, config.asset_class, config.symbols
    );

    // Build and start the application
    info!("Building trading application...");
    let app = Application::build(config.clone()).await?;

    info!("Starting trading system...");
    let handle = app.start().await?;
    info!("Trading system running.");

    // Start metrics reporter if enabled
    if config.observability_enabled {
        let metrics = Metrics::new()?;

        // Use observability_port as interval for now (repurpose the config field)
        // In future, add OBSERVABILITY_INTERVAL env var
        let interval = std::env::var("OBSERVABILITY_INTERVAL")
            .unwrap_or_else(|_| "60".to_string())
            .parse::<u64>()
            .unwrap_or(60);

        let reporter = MetricsReporter::new(handle.portfolio.clone(), metrics, interval);

        // Spawn reporter in background
        tokio::spawn(async move {
            reporter.run().await;
        });

        info!("Metrics reporter started (interval: {}s)", interval);
    } else {
        info!("Metrics reporting disabled.");
    }

    info!("Server running. Press Ctrl+C to shutdown.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received. Exiting...");

    Ok(())
}
