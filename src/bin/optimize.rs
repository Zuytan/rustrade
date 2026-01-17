//! Grid Search Parameter Optimizer Binary
//!
//! A CLI tool for running parameter grid search optimization on trading strategies.

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use clap::{Parser, Subcommand};
use rustrade::application::optimization::engine::OptimizeEngine;
use rustrade::application::optimization::optimizer::ParameterGrid;
use rustrade::application::optimization::reporting::OptimizeReporter;
use rustrade::config::StrategyMode;
use std::str::FromStr;
use tracing::info;

#[derive(Parser)]
#[command(author, version, about = "Grid Search Parameter Optimizer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run grid search optimization for a single symbol
    Run {
        /// Symbol to optimize
        #[arg(short, long, default_value = "TSLA")]
        symbol: String,

        /// Start date (YYYY-MM-DD)
        #[arg(long, default_value = "2020-01-01")]
        start: String,

        /// End date (YYYY-MM-DD)
        #[arg(long, default_value = "2023-12-31")]
        end: String,

        /// Strategy mode (standard, advanced, dynamic, trendriding, meanreversion)
        #[arg(long, default_value = "advanced")]
        strategy: String,

        /// TOML file with parameter grid configuration
        #[arg(long)]
        grid_config: Option<String>,

        /// Output JSON file for results
        #[arg(short, long, default_value = "optimization_results.json")]
        output: String,

        /// Number of top results to display
        #[arg(short, long, default_value = "10")]
        top_n: usize,
    },
    /// Run batch optimization for multiple symbols
    Batch {
        /// Comma-separated list of symbols
        #[arg(short, long, default_value = "TSLA,NVDA,AAPL")]
        symbols: String,

        /// Start date (YYYY-MM-DD)
        #[arg(long, default_value = "2020-01-01")]
        start: String,

        /// End date (YYYY-MM-DD)
        #[arg(long, default_value = "2023-12-31")]
        end: String,

        /// Strategy mode
        #[arg(long, default_value = "advanced")]
        strategy: String,

        /// Number of top results per symbol
        #[arg(short, long, default_value = "5")]
        top_n: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    let cli = Cli::parse();
    let engine = OptimizeEngine::new()?;
    let reporter = OptimizeReporter::default();

    match cli.command {
        Commands::Run {
            symbol,
            start,
            end,
            strategy,
            grid_config,
            output,
            top_n,
        } => {
            let strategy_mode = StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Advanced);

            reporter.print_header(
                &symbol,
                &start,
                &end,
                &format!("{:?}", strategy_mode),
                &output,
            );

            // Load parameter grid
            let parameter_grid = if let Some(config_file) = grid_config {
                info!("Loading parameter grid from: {}", config_file);
                load_grid_from_toml(&config_file)?
            } else {
                info!("Using default parameter grid");
                ParameterGrid::default()
            };

            reporter.print_grid_info(&parameter_grid);
            println!("{}\n", "=".repeat(80));

            // Parse dates
            let (start_dt, end_dt) = parse_date_range(&start, &end)?;

            // Run optimization
            println!("🚀 Starting optimization...\n");
            let results = engine
                .run_grid_search(&symbol, start_dt, end_dt, strategy_mode, parameter_grid)
                .await?;

            // Display and export results
            let top_results = engine.rank_results(results.clone(), top_n);
            reporter.print_results_table(&top_results, top_n);

            if let Some(best) = top_results.first() {
                reporter.print_best_config(best);
            }

            reporter.export_json(&results, &output)?;
            println!("✅ Optimization complete!\n");
        }
        Commands::Batch {
            symbols,
            start,
            end,
            strategy,
            top_n,
        } => {
            let symbol_list: Vec<String> =
                symbols.split(',').map(|s| s.trim().to_string()).collect();
            let strategy_mode = StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Advanced);
            let parameter_grid = ParameterGrid::default();

            println!("{}", "=".repeat(80));
            println!("🔍 BATCH GRID SEARCH OPTIMIZER");
            println!("Symbols: {:?}", symbol_list);
            println!("Period: {} to {}", start, end);
            println!("Strategy: {:?}", strategy_mode);
            println!("{}\n", "=".repeat(80));

            let (start_dt, end_dt) = parse_date_range(&start, &end)?;

            let batch_results = engine
                .run_batch(symbol_list, start_dt, end_dt, strategy_mode, parameter_grid)
                .await;

            for (symbol, result) in batch_results {
                match result {
                    Ok(results) => {
                        let top_results = engine.rank_results(results.clone(), top_n);
                        println!("\n📈 {} - Top {} Results:", symbol, top_n);
                        reporter.print_results_table(&top_results, top_n);

                        let filename = format!("{}_optimization.json", symbol.to_lowercase());
                        if let Err(e) = reporter.export_json(&results, &filename) {
                            eprintln!("Warning: Failed to export {}: {}", filename, e);
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Error optimizing {}: {}", symbol, e);
                    }
                }
            }

            println!("\n✅ Batch optimization complete!\n");
        }
    }

    Ok(())
}

/// Parses start and end date strings into DateTime<Utc>.
fn parse_date_range(
    start: &str,
    end: &str,
) -> Result<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .context(format!("Invalid start date format: {}", start))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .context(format!("Invalid end date format: {}", end))?;

    let start_dt = Utc
        .from_local_datetime(
            &start_date
                .and_hms_opt(14, 30, 0)
                .context("Invalid start time")?,
        )
        .single()
        .context("Failed to create start datetime")?;
    let end_dt = Utc
        .from_local_datetime(&end_date.and_hms_opt(21, 0, 0).context("Invalid end time")?)
        .single()
        .context("Failed to create end datetime")?;

    Ok((start_dt, end_dt))
}

/// Loads a parameter grid from a TOML file.
fn load_grid_from_toml(path: &str) -> Result<ParameterGrid> {
    let content = std::fs::read_to_string(path)
        .context(format!("Failed to read grid config file: {}", path))?;
    let grid: ParameterGrid =
        toml::from_str(&content).context(format!("Failed to parse grid config TOML: {}", path))?;
    Ok(grid)
}
