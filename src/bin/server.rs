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
use tracing::info;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Setup log broadcast channel for WebSocket API
    let (log_tx, _) = tokio::sync::broadcast::channel::<String>(1000);
    let broadcast_log_layer = rustrade::infrastructure::api::BroadcastLogLayer::new(log_tx.clone());

    // Setup logging (stdout only, no UI channel needed)
    let stdout_layer = tracing_subscriber::fmt::layer().with_target(false).pretty();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(broadcast_log_layer)
        .init();

    info!("Rustrade Server {} starting...", env!("CARGO_PKG_VERSION"));
    info!("Mode: HEADLESS (no UI)");
    info!("Metrics: Push-based (JSON to stdout)");

    // Load configuration
    let config = Config::from_env()?;
    info!(
        "Configuration loaded: Mode={:?}, Asset={:?}, Symbols={:?}",
        config.mode, config.asset_class, config.platform.symbols
    );

    // Build and start the application
    info!("Building trading application...");
    let app = Application::build(config.clone()).await?;

    info!("Starting trading system...");
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let (handle, mut join_set) = app.start(cancel_token.clone()).await?;
    info!("Trading system running.");

    // Start API Server
    let api_port = std::env::var("PORT")
        .or_else(|_| std::env::var("API_PORT"))
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    let portfolio_clone = handle.portfolio.clone();
    let exec_service_clone = handle.execution_service.clone();
    let log_tx_clone = log_tx.clone();

    tokio::spawn(async move {
        if let Err(e) = rustrade::infrastructure::api::run_api_server(
            api_port,
            portfolio_clone,
            exec_service_clone,
            log_tx_clone,
        )
        .await
        {
            tracing::error!("API Server error: {}", e);
        }
    });

    // Start metrics reporter if enabled
    if config.observability.enabled {
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

    tokio::select! {
        _ = cancel_token.cancelled() => {
            info!("Cancellation requested. Waiting for tasks to complete...");
        }
        res = join_set.join_next() => {
            if let Some(res) = res {
                if let Err(e) = res {
                    tracing::error!("A background task panicked or was aborted: {}", e);
                }
                cancel_token.cancel();
            }
        }
    }

    info!("Shutting down background tasks...");
    while let Some(res) = join_set.join_next().await {
        if let Err(e) = res {
            tracing::error!("Background task error during shutdown: {}", e);
        }
    }
    info!("All background tasks completed. Server exiting.");

    Ok(())
}
