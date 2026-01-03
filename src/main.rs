use rustrade::application::agents::user_agent::UserAgent;
use rustrade::application::system::Application;
use rustrade::config::Config;
// use rustrade::interfaces::ui; // Unused
use tracing::{info, Level};
use tracing_subscriber::prelude::*;

// A writer that sends logs to the UI via a crossbeam channel
struct ChannelWriter {
    sender: crossbeam_channel::Sender<String>,
}

impl std::io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let msg = String::from_utf8_lossy(buf).to_string();
        // Strip ANSI codes if necessary (tracing-subscriber can be configured to not output them)
        let _ = self.sender.try_send(msg);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// Cloneable wrapper for MakeWriter
#[derive(Clone)]
struct ChannelWriterFactory {
    sender: crossbeam_channel::Sender<String>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ChannelWriterFactory {
    type Writer = ChannelWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ChannelWriter {
            sender: self.sender.clone(),
        }
    }
}

fn main() -> anyhow::Result<()> {
    // 0. Load Env (before starting anything)
    dotenv::dotenv().ok(); // Load .env file

    // 1. Create Log Channel
    let (log_tx, log_rx) = crossbeam_channel::unbounded();

    // 2. Setup Logging (Stdout + UI)
    // We use a registry to add multiple layers
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(false) // cleaner
        .pretty();

    let ui_layer = tracing_subscriber::fmt::layer()
        .with_writer(ChannelWriterFactory { sender: log_tx })
        .with_ansi(false) // No color codes for UI text
        .with_target(false); // cleaner

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .with(stdout_layer)
        .with(ui_layer)
        .init();

    info!("Initializing Rustrade Native Agent...");

    // 3. Create Tokio Runtime in a background thread
    let (system_tx, system_rx) = crossbeam_channel::bounded(1);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime");

        rt.block_on(async move {
            info!("Background Runtime Started.");

            // Load Config
            let config = match Config::from_env() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to load config: {}", e);
                    return;
                }
            };

            // Build Application
            let app = match Application::build(config).await {
                Ok(app) => app,
                Err(e) => {
                    tracing::error!("Failed to build application: {}", e);
                    return;
                }
            };

            // Start System
            match app.start().await {
                Ok(handle) => {
                    let _ = system_tx.send(handle);
                    info!("Trading System Running.");
                    // Keep the background runtime alive by awaiting a pending future or parking?
                    // app.start() spawned tasks, so we just need to keep this block alive.
                    // The spawned tasks are detached, but the runtime must not drop.

                    // We can just sleep forever or await a shutdown signal.
                    // For now, let's just park.
                    std::future::pending::<()>().await;
                }
                Err(e) => {
                    tracing::error!("Failed to start application: {}", e);
                }
            }
        });
    });

    // 4. Wait for System Handle (with a timeout/loading state? No, we block main thread briefly)
    info!("Waiting for System to boot...");
    let system_handle = system_rx
        .recv()
        .expect("Failed to receive system handle (did background thread panic?)");
    info!("System Connected. Launching UI.");

    // 5. Initialize User Agent
    let agent = UserAgent::new(
        log_rx,
        system_handle.candle_rx,
        system_handle.sentinel_cmd_tx,
        system_handle.proposal_tx,
        system_handle.portfolio,
        system_handle.strategy_mode,
    );

    // 6. Run UI (Blocks Main Thread)
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Rustrade Agent"),
        ..Default::default()
    };

    eframe::run_native(
        "Rustrade Agent",
        native_options,
        Box::new(|_cc| Ok(Box::new(agent))),
    )
    .map_err(|e| anyhow::anyhow!("Eframe error: {}", e))?;

    Ok(())
}
