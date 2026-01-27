use chrono::{NaiveDate, TimeZone, Utc};
use clap::Parser;
use rustrade::application::benchmarking::engine::BenchmarkEngine;
use rustrade::config::StrategyMode;
use std::str::FromStr;
use tracing::info;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Symbol(s) to generate data for (comma separated)
    #[arg(short, long, default_value = "BTCUSD,ETHUSD,TSLA,NVDA,AAPL")]
    symbols: String,

    /// Start date (YYYY-MM-DD)
    #[arg(long, default_value = "2024-01-01")]
    start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long, default_value = "2024-12-31")]
    end: String,

    /// Lookback days (if specified, overrides start date)
    #[arg(short, long)]
    days: Option<i64>,

    /// Strategy to use for the simulation
    #[arg(long, default_value = "standard")]
    strategy: String,

    /// Asset class (stock or crypto)
    #[arg(long, default_value = "stock")]
    asset_class: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup logging
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    let cli = Cli::parse();

    // Force enable ML data collection in env for the benchmark engine to pick up
    unsafe {
        std::env::set_var("ENABLE_ML_DATA_COLLECTION", "true");
        std::env::set_var("ASSET_CLASS", &cli.asset_class);
    }

    let engine = BenchmarkEngine::new().await;

    let mut symbol_list: Vec<String> = cli
        .symbols
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    // If crypto, normalize symbols (e.g. BTCUSD -> BTC/USD)
    if cli.asset_class.to_lowercase() == "crypto" {
        symbol_list = symbol_list
            .into_iter()
            .map(|s| {
                if !s.contains('/') {
                    rustrade::domain::trading::types::normalize_crypto_symbol(&s).unwrap_or_else(
                        |_| {
                            info!(
                                "Warning: Could not normalize crypto symbol {}, using as-is",
                                s
                            );
                            s
                        },
                    )
                } else {
                    s
                }
            })
            .collect();
    }
    let end_date = NaiveDate::parse_from_str(&cli.end, "%Y-%m-%d")?;
    let end_dt = Utc.from_utc_datetime(&end_date.and_hms_opt(23, 59, 59).unwrap());

    let start_dt = if let Some(days) = cli.days {
        end_dt - chrono::Duration::days(days)
    } else {
        let start_date = NaiveDate::parse_from_str(&cli.start, "%Y-%m-%d")?;
        Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap())
    };

    let strat_mode = StrategyMode::from_str(&cli.strategy).unwrap_or(StrategyMode::Standard);

    info!("🚀 GENERATING ML TRAINING DATA");
    info!("Symbols: {:?}", symbol_list);
    info!("Period: {} to {}", start_dt, end_dt);
    info!("Strategy: {:?}", strat_mode);
    info!("Output: data/ml/training_data.csv");
    info!("{}", "=".repeat(80));

    for sym in symbol_list {
        info!("Processing {}...", sym);
        match engine
            .run_single(&sym, start_dt, end_dt, strat_mode, None)
            .await
        {
            Ok(_) => info!("✅ Done for {}", sym),
            Err(e) => info!("❌ Error for {}: {}", sym, e),
        }
    }

    info!("{}", "=".repeat(80));
    info!("✨ All simulations complete. Check data/ml/training_data.csv");

    Ok(())
}
