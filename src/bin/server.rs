//! Rustrade Server - Headless trading system
//!
//! This binary runs the trading system without a GUI, suitable for
//! server deployments. Metrics are pushed via structured JSON logs to stdout.
//!
//! # Usage
//! ```sh
//! OBSERVABILITY_INTERVAL=60 cargo run --bin server
//! ```
//!
//! # Environment Variables
//! - `OBSERVABILITY_ENABLED` - Enable metrics reporting (default: true)
//! - `OBSERVABILITY_INTERVAL` - Interval in seconds between metric outputs (default: 60)

use anyhow::Result;
use rustrade::application::system::Application;
use rustrade::config::Config;
use rustrade::infrastructure::observability::MetricsReporter;
use tracing::{Level, info};
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Setup logging (stdout only, no UI channel needed)
    let stdout_layer = tracing_subscriber::fmt::layer().with_target(false).pretty();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .with(stdout_layer)
        .init();

    info!("Rustrade Server {} starting...", env!("CARGO_PKG_VERSION"));
    info!("Mode: HEADLESS (no UI)");
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
        let metrics = handle.metrics.clone();

        let interval = std::env::var("OBSERVABILITY_INTERVAL")
            .unwrap_or_else(|_| "60".to_string())
            .parse::<u64>()
            .unwrap_or(60);

        let reporter = MetricsReporter::new(handle.portfolio.clone(), metrics, interval);

        tokio::spawn(async move {
            reporter.run().await;
        });

        info!("Metrics reporter started (interval: {}s)", interval);
    } else {
        info!("Metrics reporting disabled.");
    }

    info!("Server running. Press Ctrl+C to shutdown.");

    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received. Exiting...");

    Ok(())
}
