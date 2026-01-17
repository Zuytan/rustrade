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
use rustrade::domain::risk::optimal_parameters::OptimalParameters;
use rustrade::domain::risk::risk_appetite::RiskProfile;
use rustrade::infrastructure::optimal_parameters_persistence::OptimalParametersPersistence;
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
    /// Discover and save optimal parameters for all risk levels
    DiscoverOptimal {
        /// Symbol to use for optimization
        #[arg(short, long, default_value = "AAPL")]
        symbol: String,

        /// Start date (YYYY-MM-DD)
        #[arg(long, default_value = "2020-01-01")]
        start: String,

        /// End date (YYYY-MM-DD)
        #[arg(long, default_value = "2023-12-31")]
        end: String,

        /// Strategy mode
        #[arg(long, default_value = "advanced")]
        strategy: String,
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
        Commands::DiscoverOptimal {
            symbol,
            start,
            end,
            strategy,
        } => {
            let strategy_mode = StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Advanced);
            let (start_dt, end_dt) = parse_date_range(&start, &end)?;
            let persistence = OptimalParametersPersistence::new()?;

            println!("{}", "=".repeat(80));
            println!("🎯 DISCOVER OPTIMAL PARAMETERS FOR ALL RISK LEVELS");
            println!("Symbol: {}", symbol);
            println!("Period: {} to {}", start, end);
            println!("Strategy: {:?}", strategy_mode);
            println!("{}\n", "=".repeat(80));

            let profiles = [
                RiskProfile::Conservative,
                RiskProfile::Balanced,
                RiskProfile::Aggressive,
            ];

            for profile in profiles {
                let profile_name = format!("{:?}", profile);
                println!("\n🔍 Optimizing for {} profile...", profile_name);

                let grid = get_grid_for_profile(profile);
                let combo_count = calculate_grid_combinations(&grid);
                println!("   Testing {} parameter combinations", combo_count);

                let results = engine
                    .run_grid_search(&symbol, start_dt, end_dt, strategy_mode, grid)
                    .await?;

                if let Some(best) = engine.rank_results(results, 1).into_iter().next() {
                    let optimal = OptimalParameters::new(
                        profile,
                        best.params.fast_sma_period,
                        best.params.slow_sma_period,
                        best.params.rsi_threshold,
                        best.params.trailing_stop_atr_multiplier,
                        best.params.trend_divergence_threshold,
                        best.params.order_cooldown_seconds,
                        symbol.clone(),
                        best.sharpe_ratio,
                        best.total_return,
                        best.max_drawdown,
                        best.win_rate,
                        best.total_trades,
                    );

                    println!(
                        "   ✅ {}: fast={}, slow={}, rsi={:.0}, atr_mult={:.1}",
                        profile_name,
                        optimal.fast_sma_period,
                        optimal.slow_sma_period,
                        optimal.rsi_threshold,
                        optimal.trailing_stop_atr_multiplier
                    );
                    println!(
                        "      Sharpe={:.2}, Return={:.1}%, Drawdown={:.1}%",
                        optimal.sharpe_ratio, optimal.total_return, optimal.max_drawdown
                    );

                    persistence.upsert(optimal)?;
                } else {
                    println!("   ⚠️ No valid results for {} profile", profile_name);
                }
            }

            println!("\n✅ Optimal parameters saved to ~/.rustrade/optimal_parameters.json\n");
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

/// Returns a parameter grid tailored for a specific risk profile.
///
/// - Conservative: Tighter ranges, lower risk parameters
/// - Balanced: Middle-ground ranges
/// - Aggressive: Wider ranges, higher risk parameters
fn get_grid_for_profile(profile: RiskProfile) -> ParameterGrid {
    match profile {
        RiskProfile::Conservative => ParameterGrid {
            fast_sma: vec![10, 15, 20],
            slow_sma: vec![50, 60, 80],
            rsi_threshold: vec![55.0, 60.0, 65.0],
            trend_divergence_threshold: vec![0.002, 0.003, 0.005],
            trailing_stop_atr_multiplier: vec![1.5, 2.0, 2.5],
            order_cooldown_seconds: vec![300, 600, 900],
        },
        RiskProfile::Balanced => ParameterGrid {
            fast_sma: vec![15, 20, 25],
            slow_sma: vec![50, 60, 100],
            rsi_threshold: vec![60.0, 65.0, 70.0],
            trend_divergence_threshold: vec![0.003, 0.005, 0.008],
            trailing_stop_atr_multiplier: vec![2.5, 3.0, 4.0],
            order_cooldown_seconds: vec![0, 300, 600],
        },
        RiskProfile::Aggressive => ParameterGrid {
            fast_sma: vec![20, 25, 30],
            slow_sma: vec![60, 80, 100],
            rsi_threshold: vec![65.0, 70.0, 75.0],
            trend_divergence_threshold: vec![0.005, 0.008, 0.01],
            trailing_stop_atr_multiplier: vec![3.5, 4.5, 6.0],
            order_cooldown_seconds: vec![0, 60, 180],
        },
    }
}

/// Calculates the number of parameter combinations in a grid.
fn calculate_grid_combinations(grid: &ParameterGrid) -> usize {
    let mut count = 0;
    for fast in &grid.fast_sma {
        for slow in &grid.slow_sma {
            if fast >= slow {
                continue;
            }
            count += grid.rsi_threshold.len()
                * grid.trend_divergence_threshold.len()
                * grid.trailing_stop_atr_multiplier.len()
                * grid.order_cooldown_seconds.len();
        }
    }
    count
}
