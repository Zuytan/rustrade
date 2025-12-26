use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;
use rustrade::config::Config;
use rustrade::application::system::Application;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok(); // Load .env file
    
    // 1. Load Configuration
    let config = Config::from_env()?;
    
    // 2. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    info!("Starting Rustrade...");

    // 3. Build and Run Application
    let app = Application::build(config).await?;
    app.run().await?;

    Ok(())
}
