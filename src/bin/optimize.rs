//! Parameter Optimizer Binary
//!
//! A CLI tool for running parameter optimization using a genetic algorithm
//! (bounds derived from parameter grid or defaults).
//!
//! For long runs, use an optimized build: `cargo run --release --bin optimize -- run ...`

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use clap::{Parser, Subcommand};
use rust_decimal_macros::dec;
use rustrade::application::optimization::crypto_clusters::{default_clusters, resolve_clusters};
use rustrade::application::optimization::engine::OptimizeEngine;
use rustrade::application::optimization::optimizer::OptimizationResult;
use rustrade::application::optimization::optimizer::ParameterGrid;
use rustrade::application::optimization::reporting::OptimizeReporter;
use rustrade::config::StrategyMode;
use rustrade::domain::risk::optimal_parameters::{AssetType, OptimalParameters};
use rustrade::domain::risk::risk_appetite::RiskProfile;
use rustrade::infrastructure::optimal_parameters_persistence::OptimalParametersPersistence;
use std::str::FromStr;
use tracing::info;

#[derive(Parser)]
#[command(author, version, about = "Parameter optimizer (genetic algorithm)", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run genetic optimization for a single symbol
    Run {
        /// Symbol to optimize (e.g. TSLA, or BTC/USD for crypto)
        #[arg(short, long, default_value = "TSLA")]
        symbol: String,

        /// Asset type: stock or crypto (sets default session times; use with default symbol)
        #[arg(long, default_value = "stock")]
        asset_type: String,

        /// Start date (YYYY-MM-DD)
        #[arg(long, default_value = "2020-01-01")]
        start: String,

        /// End date (YYYY-MM-DD)
        #[arg(long, default_value = "2023-12-31")]
        end: String,

        /// Strategy mode (standard, advanced, dynamic, trendriding, meanreversion)
        #[arg(long, default_value = "advanced")]
        strategy: String,

        /// TOML file with parameter grid (e.g. grid.toml, grid_entry_opt.toml in project root)
        #[arg(long)]
        grid_config: Option<String>,

        /// Train ratio: 0.5–0.9 = walk-forward (train+test). Use 1.0 or --single-period for one backtest on full range (faster).
        #[arg(long, default_value = "0.70")]
        train_ratio: f64,

        /// Single period: one backtest on full range (no train/test). Overrides train_ratio to 1.0.
        #[arg(long)]
        single_period: bool,

        /// Genetic: population size (default 24)
        #[arg(long)]
        population: Option<usize>,

        /// Genetic: number of generations (default 15)
        #[arg(long)]
        generations: Option<usize>,

        /// Genetic: mutation rate 0.0–1.0 (default 0.15)
        #[arg(long)]
        mutation_rate: Option<f64>,

        /// Bar timeframe: 1Min (default), 5Min, 15Min. Coarser = fewer bars = much faster (e.g. 5Min ≈ 5× faster).
        #[arg(long, default_value = "1Min")]
        timeframe: String,

        /// Output JSON file for results
        #[arg(short, long, default_value = "optimization_results.json")]
        output: String,

        /// Number of top results to display
        #[arg(short, long, default_value = "10")]
        top_n: usize,

        /// Session start time (HH:MM:SS). Default: stocks 14:30, crypto 00:00
        #[arg(long)]
        session_start: Option<String>,

        /// Session end time (HH:MM:SS). Default: stocks 21:00, crypto 23:59
        #[arg(long)]
        session_end: Option<String>,

        /// Risk appetite score (1-9) for optimization. When set, each evaluated config is adapted to this risk (sizing, stops, take-profit). Omit to use fixed params.
        #[arg(long)]
        risk_score: Option<u8>,
    },
    /// Run batch optimization for multiple symbols
    Batch {
        /// Comma-separated list of symbols (e.g. TSLA,NVDA,AAPL or BTC/USD,ETH/USD for crypto)
        #[arg(short, long, default_value = "TSLA,NVDA,AAPL")]
        symbols: String,

        /// Asset type: stock or crypto (sets default symbols and session times when not overridden)
        #[arg(long, default_value = "stock")]
        asset_type: String,

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

        /// Session start time (HH:MM:SS). Default: stocks 14:30, crypto 00:00
        #[arg(long)]
        session_start: Option<String>,

        /// Session end time (HH:MM:SS). Default: stocks 21:00, crypto 23:59
        #[arg(long)]
        session_end: Option<String>,
    },
    /// Discover and save optimal parameters for all risk levels
    /// Uses benchmark-proven strategies: Conservative→Standard, Balanced→RegimeAdaptive, Aggressive→SMC
    DiscoverOptimal {
        /// Symbol to use (e.g. AAPL for stocks, BTC/USD for crypto)
        #[arg(short, long, default_value = "AAPL")]
        symbol: String,

        /// Asset type: stock or crypto (default symbol becomes BTC/USD when crypto)
        #[arg(short, long, default_value = "stock")]
        asset_type: String,
    },

    /// List defined crypto clusters (for use with run-clusters)
    ListClusters,

    /// Run genetic optimization per crypto cluster (one run per cluster on representative symbol)
    RunClusters {
        /// Start date (YYYY-MM-DD)
        #[arg(long, default_value = "2023-01-01")]
        start: String,

        /// End date (YYYY-MM-DD)
        #[arg(long, default_value = "2024-06-30")]
        end: String,

        /// Strategy mode
        #[arg(long, default_value = "advanced")]
        strategy: String,

        /// TOML file with parameter grid (optional; default = crypto grid)
        #[arg(long)]
        grid_config: Option<String>,

        #[arg(long)]
        population: Option<usize>,
        #[arg(long)]
        generations: Option<usize>,
        #[arg(long)]
        mutation_rate: Option<f64>,

        /// Bar timeframe: 1Min, 5Min (default), 15Min
        #[arg(long, default_value = "5Min")]
        timeframe: String,

        /// Cluster ids to run (e.g. large_cap, mid_cap). Default: all clusters
        #[arg(long, value_delimiter = ',')]
        clusters: Option<Vec<String>>,

        /// Number of top results to show per cluster
        #[arg(short, long, default_value = "5")]
        top_n: usize,

        /// Output file prefix (e.g. "out" -> out_large_cap.json, out_mid_cap.json)
        #[arg(long, default_value = "cluster")]
        output_prefix: String,

        /// Risk appetite score (1-9). When set, each evaluated config is adapted to this risk.
        #[arg(long)]
        risk_score: Option<u8>,
    },
}

#[allow(clippy::too_many_arguments)]
async fn run_optimize(
    engine: &OptimizeEngine,
    reporter: &OptimizeReporter,
    symbol: String,
    asset_type: String,
    start: String,
    end: String,
    strategy: String,
    grid_config: Option<String>,
    population: Option<usize>,
    generations: Option<usize>,
    mutation_rate: Option<f64>,
    timeframe: String,
    output: String,
    top_n: usize,
    session_start: Option<String>,
    session_end: Option<String>,
    risk_score: Option<u8>,
) -> Result<()> {
    let strategy_mode = StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Advanced);
    let is_crypto = asset_type.to_lowercase().as_str() == "crypto";
    let symbol = resolve_run_symbol(&symbol, is_crypto);
    let (session_start, session_end) =
        resolve_session_times(session_start.as_deref(), session_end.as_deref(), is_crypto);

    reporter.print_header(
        &symbol,
        &start,
        &end,
        &format!("{:?}", strategy_mode),
        &output,
    );

    let parameter_grid = if let Some(config_file) = &grid_config {
        info!("Loading parameter grid from: {}", config_file);
        load_grid_from_toml(config_file)?
    } else if is_crypto {
        info!("Using crypto default grid (responsive SMAs, shorter cooldowns for 24/7)");
        default_grid_crypto()
    } else {
        info!("Using default parameter grid (bounds for genetic algorithm)");
        ParameterGrid::default()
    };

    reporter.print_grid_info(&parameter_grid);
    println!("{}\n", "=".repeat(80));

    let (start_dt, end_dt) = parse_date_range(&start, &end, &session_start, &session_end)?;

    let population_size = population.unwrap_or(24);
    let num_generations = generations.unwrap_or(15);
    let total_evals = population_size * num_generations;
    let estimated_mins = (total_evals as f64 * 3.0 / 60.0 / 4.0).ceil() as u64;
    println!(
        "Genetic optimization: pop={}, gen={} ({} evals), estimated ~{} min",
        population_size, num_generations, total_evals, estimated_mins
    );
    println!(
        "First step: fetching market data ({} + SPY). Large date ranges can take several minutes...\n",
        timeframe
    );

    if let Some(s) = risk_score {
        if !(1..=9).contains(&s) {
            anyhow::bail!("--risk-score must be between 1 and 9, got {}", s);
        }
        println!("Risk score: {} (params adapted to this risk)\n", s);
    }

    let results = engine
        .run_genetic_optimization(
            &symbol,
            start_dt,
            end_dt,
            strategy_mode,
            parameter_grid,
            population,
            generations,
            mutation_rate,
            Some(timeframe),
            risk_score,
        )
        .await?;

    let top_results = engine.rank_results(results.clone(), top_n);
    reporter.print_results_table(&top_results, top_n);
    if let Some(best) = top_results.first() {
        reporter.print_best_config(best);
    }
    reporter.export_json(&results, &output)?;

    // Save best params to ~/.rustrade/optimal_parameters.json (used by UI and server)
    if let Some(best) = top_results.first() {
        let asset = AssetType::from_str(&asset_type).unwrap_or(AssetType::Stock);
        if let Ok(persist) = OptimalParametersPersistence::new() {
            match risk_score {
                Some(score) => {
                    let opt = best.to_optimal_parameters(score, asset, symbol.clone());
                    if persist.upsert(opt).is_ok() {
                        println!(
                            "Saved best params for risk {} to ~/.rustrade/optimal_parameters.json",
                            score
                        );
                    }
                }
                None => {
                    for profile in [
                        RiskProfile::Conservative,
                        RiskProfile::Balanced,
                        RiskProfile::Aggressive,
                    ] {
                        let opt =
                            best.to_optimal_parameters_for_profile(profile, asset, symbol.clone());
                        persist.upsert(opt).ok();
                    }
                    println!(
                        "Saved best params for all risk profiles to ~/.rustrade/optimal_parameters.json"
                    );
                }
            }
        }
    }

    println!("Optimization complete!\n");
    Ok(())
}

/// Runs genetic optimization once per crypto cluster (on representative symbol). Session 24/7.
#[allow(clippy::too_many_arguments)]
async fn run_clusters(
    engine: &OptimizeEngine,
    reporter: &OptimizeReporter,
    start: String,
    end: String,
    strategy: String,
    grid_config: Option<String>,
    population: Option<usize>,
    generations: Option<usize>,
    mutation_rate: Option<f64>,
    timeframe: String,
    cluster_ids: Vec<String>,
    top_n: usize,
    output_prefix: String,
    risk_score: Option<u8>,
) -> Result<()> {
    let strategy_mode = StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Advanced);
    let (session_start, session_end) = ("00:00:00".to_string(), "23:59:59".to_string());
    let clusters = resolve_clusters(&cluster_ids);
    if clusters.is_empty() {
        anyhow::bail!("No clusters found. Use list-clusters to see available ids.");
    }

    let parameter_grid = if let Some(ref config_file) = grid_config {
        info!("Loading parameter grid from: {}", config_file);
        load_grid_from_toml(config_file)?
    } else if strategy_mode == StrategyMode::Ensemble {
        info!("Using Ensemble (modern) grid for clusters");
        grid_for_ensemble_crypto()
    } else {
        info!("Using crypto default grid for clusters");
        default_grid_crypto()
    };

    let (start_dt, end_dt) = parse_date_range(&start, &end, &session_start, &session_end)?;

    println!("{}", "=".repeat(80));
    println!("🔀 CRYPTO CLUSTER OPTIMIZATION");
    println!(
        "Clusters: {}",
        clusters.iter().map(|c| c.id).collect::<Vec<_>>().join(", ")
    );
    println!("Period: {} to {} (24/7)", start, end);
    println!("Strategy: {:?}", strategy_mode);
    if let Some(s) = risk_score {
        println!("Risk score: {} (params adapted to this risk)", s);
    }
    reporter.print_grid_info(&parameter_grid);
    println!("{}\n", "=".repeat(80));

    // Best result across all clusters (for saving to ~/.rustrade when --risk-score is set)
    let mut best_overall: Option<(OptimizationResult, String)> = None;

    for cluster in &clusters {
        let symbol = cluster.representative_symbol().to_string();
        println!(
            "\n📂 Cluster \"{}\" ({}), representative: {}",
            cluster.id, cluster.label, symbol
        );

        let results = engine
            .run_genetic_optimization(
                &symbol,
                start_dt,
                end_dt,
                strategy_mode,
                parameter_grid.clone(),
                population,
                generations,
                mutation_rate,
                Some(timeframe.clone()),
                risk_score,
            )
            .await?;

        let top_results = engine.rank_results(results.clone(), top_n);
        reporter.print_results_table(&top_results, top_n);
        if let Some(best) = top_results.first() {
            reporter.print_best_config(best);
            if best_overall
                .as_ref()
                .is_none_or(|(prev, _)| best.objective_score > prev.objective_score)
            {
                best_overall = Some((best.clone(), symbol.clone()));
            }
        }

        let filename = format!("{}_{}_optimization.json", output_prefix, cluster.id);
        reporter.export_json(&results, &filename)?;
        println!("   Exported {} results to {}", results.len(), filename);
    }

    if let Some((best, symbol_used)) = best_overall
        && let Ok(persist) = OptimalParametersPersistence::new()
    {
        match risk_score {
            Some(score) => {
                let opt = best.to_optimal_parameters(score, AssetType::Crypto, symbol_used.clone());
                if persist.upsert(opt).is_ok() {
                    println!(
                        "Saved best params for risk {} to ~/.rustrade/optimal_parameters.json (Crypto)",
                        score
                    );
                }
            }
            None => {
                for profile in [
                    RiskProfile::Conservative,
                    RiskProfile::Balanced,
                    RiskProfile::Aggressive,
                ] {
                    let opt = best.to_optimal_parameters_for_profile(
                        profile,
                        AssetType::Crypto,
                        symbol_used.clone(),
                    );
                    persist.upsert(opt).ok();
                }
                println!(
                    "Saved best params for all risk profiles to ~/.rustrade/optimal_parameters.json (Crypto)"
                );
            }
        }
    }

    println!("\n✅ Cluster optimization complete.\n");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Suppress "trade rejected" / validation logs unless RUST_LOG requests them (e.g. =debug)
    let base = std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string());
    let filter_str = format!(
        "{},\
         rustrade::application::optimization::optimizer=info,\
         rustrade::application::risk_management=warn,\
         rustrade::application::risk_management::pipeline=warn,\
         rustrade::application::trading=warn,\
         rustrade::domain::trading::events=warn",
        base
    );
    let filter = tracing_subscriber::EnvFilter::try_new(&filter_str)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(filter)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    let cli = Cli::parse();
    let engine = OptimizeEngine::new()?;
    let reporter = OptimizeReporter::default();

    match cli.command {
        Commands::Run {
            symbol,
            asset_type,
            start,
            end,
            strategy,
            grid_config,
            train_ratio: _,
            single_period: _,
            population,
            generations,
            mutation_rate,
            timeframe,
            output,
            top_n,
            session_start,
            session_end,
            risk_score,
        } => {
            run_optimize(
                &engine,
                &reporter,
                symbol,
                asset_type,
                start,
                end,
                strategy,
                grid_config,
                population,
                generations,
                mutation_rate,
                timeframe,
                output,
                top_n,
                session_start,
                session_end,
                risk_score,
            )
            .await?
        }
        Commands::Batch {
            symbols,
            asset_type,
            start,
            end,
            strategy,
            top_n,
            session_start,
            session_end,
        } => {
            let is_crypto = asset_type.to_lowercase().as_str() == "crypto";
            let symbol_list = resolve_batch_symbols(&symbols, is_crypto);
            let (session_start, session_end) =
                resolve_session_times(session_start.as_deref(), session_end.as_deref(), is_crypto);
            let strategy_mode = StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Advanced);
            let parameter_grid = ParameterGrid::default();

            println!("{}", "=".repeat(80));
            println!("🔍 BATCH GRID SEARCH OPTIMIZER");
            println!("Symbols: {:?}", symbol_list);
            println!("Period: {} to {}", start, end);
            println!("Strategy: {:?}", strategy_mode);
            println!("{}\n", "=".repeat(80));

            let (start_dt, end_dt) = parse_date_range(&start, &end, &session_start, &session_end)?;

            let batch_results = engine
                .run_batch(
                    symbol_list,
                    start_dt,
                    end_dt,
                    strategy_mode,
                    parameter_grid,
                    0.70,
                )
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
        Commands::DiscoverOptimal { symbol, asset_type } => {
            use rustrade::domain::risk::optimal_parameters::AssetType;

            let asset = AssetType::from_str(&asset_type).unwrap_or(AssetType::Stock);
            let symbol = resolve_discover_symbol(&symbol, asset);
            let persistence = OptimalParametersPersistence::new()?;

            // Define periods based on asset type (crypto: 24/7, use multi-day windows)
            let periods: Vec<(&str, &str)> = match asset {
                AssetType::Stock => vec![
                    ("2022-06-01", "2022-06-30"), // Bear market
                    ("2023-01-01", "2023-01-31"), // Recovery
                    ("2023-07-01", "2023-07-31"), // Summer rally
                    ("2024-03-01", "2024-03-31"), // Q1 2024
                    ("2024-11-01", "2024-11-30"), // Post-election
                    ("2025-01-01", "2025-01-17"), // Recent
                ],
                AssetType::Crypto => vec![
                    ("2023-10-01", "2023-10-31"), // Pre-ETF
                    ("2024-01-01", "2024-01-31"), // ETF approval
                    ("2024-06-01", "2024-06-30"), // Mid-year
                    ("2024-10-01", "2024-10-31"), // Pre-election
                    ("2024-12-01", "2024-12-31"), // Year-end
                ],
            };

            let period_desc = match asset {
                AssetType::Stock => "6 monthly windows (2022-2025)",
                AssetType::Crypto => "5 monthly windows (2023-2024)",
            };

            println!("{}", "=".repeat(80));
            println!("🎯 DISCOVER OPTIMAL PARAMETERS (Multi-Period Analysis)");
            println!("Symbol: {} ({})", symbol, asset);
            println!("Periods: {}", period_desc);
            println!(
                "Strategy per profile: Conservative→Standard, Balanced→RegimeAdaptive, Aggressive→SMC"
            );
            println!("{}\n", "=".repeat(80));

            let profiles = [
                RiskProfile::Conservative,
                RiskProfile::Balanced,
                RiskProfile::Aggressive,
            ];

            for profile in profiles {
                let profile_name = format!("{:?}", profile);
                let strategy_mode = get_strategy_for_profile(profile);
                println!(
                    "\n🔍 Optimizing {} {} with {:?} strategy...",
                    asset, profile_name, strategy_mode
                );

                let grid = get_grid_for_profile(profile, asset);
                let combo_count = calculate_grid_combinations(&grid);
                println!(
                    "   Testing {} combinations across {} periods",
                    combo_count,
                    periods.len()
                );

                // Collect results from all periods
                let mut all_results = Vec::new();
                for (start, end) in &periods {
                    // Use default times for DiscoverOptimal (Stock: 14:30-21:00, Crypto: 00:00-23:59)
                    // But here we rely on parse_date_range defaults if we passed only dates.
                    // Actually, we need to pass times.
                    // For now, let's just use the defaults associated with Stock for safety,
                    // or hardcode based on asset_type.
                    let (s_time, e_time) = match asset {
                        AssetType::Stock => ("14:30:00", "21:00:00"),
                        AssetType::Crypto => ("00:00:00", "23:59:59"),
                    };
                    let (start_dt, end_dt) = parse_date_range(start, end, s_time, e_time)?;
                    let results = engine
                        .run_grid_search(
                            &symbol,
                            start_dt,
                            end_dt,
                            strategy_mode,
                            grid.clone(),
                            0.70,
                            None,
                        )
                        .await?;
                    all_results.extend(results);
                }

                // Rank across all periods
                if let Some(best) = engine.rank_results(all_results, 1).into_iter().next() {
                    let optimal = OptimalParameters::new(
                        asset,
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
                        "   ✅ {} {}: fast={}, slow={}, rsi={:.0}, atr_mult={:.1}",
                        asset,
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
                    println!(
                        "   ⚠️ No valid results for {} {} profile",
                        asset, profile_name
                    );
                }
            }

            println!("\n✅ Optimal parameters saved to ~/.rustrade/optimal_parameters.json\n");
        }
        Commands::ListClusters => {
            println!("Crypto clusters (use with run-clusters --clusters <id>):\n");
            for c in default_clusters() {
                println!("  {}  {}", c.id, c.label);
                println!("      symbols: {}", c.symbols.join(", "));
                println!("      representative: {}", c.representative_symbol());
            }
        }
        Commands::RunClusters {
            start,
            end,
            strategy,
            grid_config,
            population,
            generations,
            mutation_rate,
            timeframe,
            clusters,
            top_n,
            output_prefix,
            risk_score,
        } => {
            run_clusters(
                &engine,
                &reporter,
                start,
                end,
                strategy,
                grid_config,
                population,
                generations,
                mutation_rate,
                timeframe,
                clusters.unwrap_or_default(),
                top_n,
                output_prefix,
                risk_score,
            )
            .await?;
        }
    }

    Ok(())
}

/// Default symbol for run when asset is crypto and user kept stock default.
fn resolve_run_symbol(symbol: &str, is_crypto: bool) -> String {
    if is_crypto && (symbol == "TSLA" || symbol == "AAPL") {
        "BTC/USD".to_string()
    } else {
        symbol.to_string()
    }
}

/// Default symbols for batch when asset is crypto.
fn resolve_batch_symbols(symbols: &str, is_crypto: bool) -> Vec<String> {
    if is_crypto && symbols.contains("TSLA") {
        vec!["BTC/USD".to_string(), "ETH/USD".to_string()]
    } else {
        symbols.split(',').map(|s| s.trim().to_string()).collect()
    }
}

/// Default symbol for discover-optimal when asset is crypto and user kept stock default.
fn resolve_discover_symbol(
    symbol: &str,
    asset: rustrade::domain::risk::optimal_parameters::AssetType,
) -> String {
    if asset == rustrade::domain::risk::optimal_parameters::AssetType::Crypto
        && (symbol == "AAPL" || symbol == "TSLA")
    {
        "BTC/USD".to_string()
    } else {
        symbol.to_string()
    }
}

/// Session times: stock 14:30-21:00, crypto 00:00-23:59 (24/7).
fn resolve_session_times(
    start: Option<&str>,
    end: Option<&str>,
    is_crypto: bool,
) -> (String, String) {
    let (s, e) = match (start, end) {
        (Some(s), Some(e)) => (s.to_string(), e.to_string()),
        _ if is_crypto => ("00:00:00".to_string(), "23:59:59".to_string()),
        _ => ("14:30:00".to_string(), "21:00:00".to_string()),
    };
    (s, e)
}

/// Parses start and end date strings into DateTime<Utc>.
fn parse_date_range(
    start: &str,
    end: &str,
    start_time: &str,
    end_time: &str,
) -> Result<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .context(format!("Invalid start date format: {}", start))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .context(format!("Invalid end date format: {}", end))?;

    let start_time_parsed = chrono::NaiveTime::parse_from_str(start_time, "%H:%M:%S")
        .context(format!("Invalid start time format: {}", start_time))?;
    let end_time_parsed = chrono::NaiveTime::parse_from_str(end_time, "%H:%M:%S")
        .context(format!("Invalid end time format: {}", end_time))?;

    let start_dt = Utc
        .from_local_datetime(&start_date.and_time(start_time_parsed))
        .single()
        .context("Failed to create start datetime")?;
    let end_dt = Utc
        .from_local_datetime(&end_date.and_time(end_time_parsed))
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

/// Default parameter grid for crypto (genetic algo bounds). More responsive SMAs, shorter cooldowns (24/7).
fn default_grid_crypto() -> ParameterGrid {
    get_grid_for_profile(
        rustrade::domain::risk::risk_appetite::RiskProfile::Balanced,
        rustrade::domain::risk::optimal_parameters::AssetType::Crypto,
    )
}

/// Grid for Ensemble strategy (StatMomentum + ZScoreMR + SMC). Sets modern param bounds for genetic search.
/// Legacy params are kept narrow; the 8 modern params drive the Ensemble's signal generation.
fn grid_for_ensemble_crypto() -> ParameterGrid {
    ParameterGrid {
        fast_sma: vec![10, 20],
        slow_sma: vec![50, 80],
        rsi_threshold: vec![dec!(60.0), dec!(70.0)],
        trend_divergence_threshold: vec![dec!(0.005), dec!(0.01)],
        trailing_stop_atr_multiplier: vec![dec!(2.0), dec!(3.0), dec!(4.0)],
        order_cooldown_seconds: vec![0, 120, 300],
        stat_momentum_lookback: Some(vec![8, 10, 14, 20]),
        stat_momentum_threshold: Some(vec![dec!(1.0), dec!(1.5), dec!(2.0), dec!(2.5)]),
        zscore_lookback: Some(vec![15, 20, 25, 30]),
        zscore_entry_threshold: Some(vec![dec!(-2.5), dec!(-2.0), dec!(-1.5)]),
        zscore_exit_threshold: Some(vec![dec!(-0.5), dec!(0.0), dec!(0.5)]),
        ofi_threshold: Some(vec![dec!(0.2), dec!(0.3), dec!(0.4)]),
        smc_ob_lookback: Some(vec![15, 20, 25, 30]),
        smc_min_fvg_size_pct: Some(vec![dec!(0.001), dec!(0.005), dec!(0.01), dec!(0.02)]),
    }
}

/// Returns a parameter grid tailored for a specific risk profile and asset type.
/// Crypto uses more responsive SMAs and shorter cooldowns (24/7 volatile markets).
fn get_grid_for_profile(
    profile: RiskProfile,
    asset: rustrade::domain::risk::optimal_parameters::AssetType,
) -> ParameterGrid {
    let is_crypto = asset == rustrade::domain::risk::optimal_parameters::AssetType::Crypto;
    match profile {
        RiskProfile::Conservative => ParameterGrid {
            fast_sma: vec![10, 15, 20],
            slow_sma: vec![50, 60, 80, 100],
            rsi_threshold: vec![dec!(54.0), dec!(58.0), dec!(62.0), dec!(66.0)],
            trend_divergence_threshold: vec![dec!(0.002), dec!(0.003), dec!(0.005)],
            trailing_stop_atr_multiplier: vec![dec!(1.5), dec!(2.0), dec!(2.5), dec!(3.0)],
            order_cooldown_seconds: if is_crypto {
                vec![60, 180, 420]
            } else {
                vec![300, 600, 900]
            },
            ..Default::default()
        },
        RiskProfile::Balanced => ParameterGrid {
            fast_sma: if is_crypto {
                vec![8, 14, 20, 26]
            } else {
                vec![12, 18, 22, 28]
            },
            slow_sma: vec![50, 65, 85, 110],
            rsi_threshold: vec![dec!(58.0), dec!(63.0), dec!(68.0), dec!(72.0)],
            trend_divergence_threshold: vec![dec!(0.003), dec!(0.005), dec!(0.007), dec!(0.01)],
            trailing_stop_atr_multiplier: vec![dec!(2.0), dec!(2.75), dec!(3.5), dec!(4.5)],
            order_cooldown_seconds: if is_crypto {
                vec![0, 120, 300]
            } else {
                vec![0, 180, 420, 600]
            },
            ..Default::default()
        },
        RiskProfile::Aggressive => ParameterGrid {
            fast_sma: if is_crypto {
                vec![10, 18, 26]
            } else {
                vec![18, 24, 30]
            },
            slow_sma: vec![55, 70, 90, 120],
            rsi_threshold: vec![dec!(62.0), dec!(67.0), dec!(72.0), dec!(76.0)],
            trend_divergence_threshold: vec![dec!(0.004), dec!(0.006), dec!(0.009), dec!(0.012)],
            trailing_stop_atr_multiplier: vec![dec!(3.0), dec!(4.0), dec!(5.0), dec!(6.0)],
            order_cooldown_seconds: if is_crypto {
                vec![0, 60]
            } else {
                vec![0, 60, 180]
            },
            ..Default::default()
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

/// Returns the optimal strategy for each risk profile based on benchmark analysis.
///
/// Mapping based on comprehensive testing across 5 symbols, 9 strategies, 3 risk levels:
/// - Conservative (1-3): Standard - Safe with ADX filters, avoids choppy markets
/// - Balanced (4-6): RegimeAdaptive - Steady gains with good risk/reward balance  
/// - Aggressive (7-10): SMC - Best alpha generator with proven robust scaling
fn get_strategy_for_profile(profile: RiskProfile) -> StrategyMode {
    match profile {
        RiskProfile::Conservative => StrategyMode::Standard,
        RiskProfile::Balanced => StrategyMode::RegimeAdaptive,
        RiskProfile::Aggressive => StrategyMode::SMC,
    }
}
