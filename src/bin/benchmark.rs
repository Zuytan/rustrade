use anyhow::Context;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use clap::{Parser, Subcommand};
use rustrade::application::agents::analyst_config::AnalystConfig;
use rustrade::application::benchmarking::engine::BenchmarkEngine;

/// One benchmark window: (label, start_dt, end_dt).
type PeriodWindow = (String, DateTime<Utc>, DateTime<Utc>);

/// Build AnalystConfig from OptimalParameters (from ~/.rustrade) and apply risk appetite for the score.
fn optimal_params_to_analyst_config(
    params: &OptimalParameters,
    score: u8,
) -> anyhow::Result<AnalystConfig> {
    let appetite = RiskAppetite::new(score).context("risk score must be 1-9")?;
    let mut cfg = AnalystConfig {
        fast_sma_period: params.fast_sma_period,
        slow_sma_period: params.slow_sma_period,
        rsi_threshold: params.rsi_threshold,
        trailing_stop_atr_multiplier: params.trailing_stop_atr_multiplier,
        trend_divergence_threshold: params.trend_divergence_threshold,
        order_cooldown_seconds: params.order_cooldown_seconds,
        ..AnalystConfig::default()
    };
    cfg.apply_risk_appetite(&appetite);
    Ok(cfg)
}

/// Parse --risk-levels "2,5,8" into Vec<u8>. Scores must be in 1..=9.
fn parse_risk_levels(s: &str) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    for part in s.split(',').map(|x| x.trim()) {
        let score: u8 = part
            .parse()
            .context("risk-levels must be comma-separated integers")?;
        if !(1..=9).contains(&score) {
            anyhow::bail!("Risk score must be between 1 and 9, got: {}", score);
        }
        out.push(score);
    }
    Ok(out)
}

/// Parse --periods "start1:end1,start2:end2" into (label, start_dt, end_dt) using same time-of-day as benchmark.
fn parse_periods(s: &str, is_crypto: bool) -> anyhow::Result<Vec<PeriodWindow>> {
    let (start_h, start_m, start_s) = if is_crypto { (0, 0, 0) } else { (14, 30, 0) };
    let (end_h, end_m, end_s) = if is_crypto { (23, 59, 59) } else { (21, 0, 0) };
    let mut out = Vec::new();
    for part in s.split(',').map(|x| x.trim()) {
        let mut split = part.split(':');
        let start_str = split
            .next()
            .context("period segment must be start:end")?
            .trim();
        let end_str = split
            .next()
            .context("period segment must be start:end")?
            .trim();
        let start_date = NaiveDate::parse_from_str(start_str, "%Y-%m-%d")?;
        let end_date = NaiveDate::parse_from_str(end_str, "%Y-%m-%d")?;
        let start_dt =
            Utc.from_utc_datetime(&start_date.and_hms_opt(start_h, start_m, start_s).unwrap());
        let end_dt = Utc.from_utc_datetime(&end_date.and_hms_opt(end_h, end_m, end_s).unwrap());
        let label = format!("{} to {}", start_str, end_str);
        out.push((label, start_dt, end_dt));
    }
    Ok(out)
}
use rustrade::application::benchmarking::reporting::{BenchmarkReporter, convert_backtest_result};
use rustrade::application::optimization::optimizer::OptimizationResult;
use rustrade::config::StrategyMode;
use rustrade::domain::risk::optimal_parameters::{AssetType, OptimalParameters};
use rustrade::domain::risk::risk_appetite::RiskAppetite;
use rustrade::infrastructure::optimal_parameters_persistence::OptimalParametersPersistence;
use std::str::FromStr;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Standard benchmark run
    Run {
        /// Symbol(s) to benchmark (comma separated)
        #[arg(short, long, default_value = "TSLA")]
        symbols: String,

        /// Start date (YYYY-MM-DD)
        #[arg(long, default_value = "2024-12-20")]
        start: String,

        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: Option<String>,

        /// Lookback days (if end date not specified)
        #[arg(short, long, default_value = "30")]
        days: i64,

        /// Strategy to use
        #[arg(long, default_value = "standard")]
        strategy: String,

        /// Run in parallel
        #[arg(short, long)]
        parallel: bool,

        /// Risk Score (1-10)
        #[arg(short, long, default_value = "5")]
        risk: u8,

        /// Asset class (stock or crypto)
        #[arg(long, default_value = "stock")]
        asset_class: String,

        /// JSON file from optimize (e.g. optimization_results.json), or "rustrade" to use ~/.rustrade/optimal_parameters.json. Uses best params and runs benchmark with them.
        #[arg(long)]
        params_file: Option<String>,

        /// Multiple periods for benchmark (only with params-file). Comma-separated "start:end" (e.g. "2024-01-01:2024-03-31,2024-04-01:2024-06-30").
        #[arg(long)]
        periods: Option<String>,

        /// Multiple risk appetites (1-9). Comma-separated (e.g. "2,5,8"). When set, benchmark runs for each risk level; report shows Strategy e.g. "Ensemble Risk-5".
        #[arg(long)]
        risk_levels: Option<String>,
    },
    /// Matrix benchmark (Parameter Grid Search)
    Matrix {
        /// Symbol to test
        #[arg(short, long, default_value = "NVDA")]
        symbol: String,
    },
    /// Verify benchmark (Regression Tests)
    Verify,
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
    let engine = BenchmarkEngine::new().await;
    let reporter = BenchmarkReporter::new("benchmark_results");

    match cli.command {
        Commands::Run {
            symbols,
            start,
            end,
            days,
            strategy,
            parallel,
            risk,
            asset_class,
            params_file,
            periods,
            risk_levels,
        } => {
            unsafe {
                std::env::set_var("ASSET_CLASS", &asset_class);
            }

            let mut symbol_list: Vec<String> =
                symbols.split(',').map(|s| s.trim().to_string()).collect();

            // Normalize crypto symbols
            if asset_class.to_lowercase() == "crypto" {
                symbol_list = symbol_list
                    .into_iter()
                    .map(|s| {
                        if !s.contains('/') {
                            rustrade::domain::trading::types::normalize_crypto_symbol(&s)
                                .unwrap_or(s)
                        } else {
                            s
                        }
                    })
                    .collect();
            }
            let start_date = NaiveDate::parse_from_str(&start, "%Y-%m-%d")?;
            let is_crypto = asset_class.to_lowercase() == "crypto";
            let (start_h, start_m, start_s) = if is_crypto { (0, 0, 0) } else { (14, 30, 0) };
            let (end_h, end_m, end_s) = if is_crypto { (23, 59, 59) } else { (21, 0, 0) };

            let start_dt =
                Utc.from_utc_datetime(&start_date.and_hms_opt(start_h, start_m, start_s).unwrap());
            let end_dt = if let Some(e) = &end {
                let end_date = NaiveDate::parse_from_str(e, "%Y-%m-%d")?;
                Utc.from_utc_datetime(&end_date.and_hms_opt(end_h, end_m, end_s).unwrap())
            } else {
                start_dt + chrono::Duration::days(days)
            };

            let mut results = Vec::new();

            if let Some(ref path) = params_file {
                if path == "rustrade" {
                    // Benchmark using params from ~/.rustrade/optimal_parameters.json
                    let persist = OptimalParametersPersistence::new()
                        .context("Failed to open ~/.rustrade (optimal_parameters.json)")?;
                    let asset_type = if asset_class.to_lowercase().as_str() == "crypto" {
                        AssetType::Crypto
                    } else {
                        AssetType::Stock
                    };
                    let period_list: Vec<PeriodWindow> = if let Some(ref p) = periods {
                        parse_periods(p, is_crypto)?
                    } else {
                        vec![(
                            format!("{} to {}", start, end_dt.date_naive()),
                            start_dt,
                            end_dt,
                        )]
                    };
                    let risk_list: Vec<u8> = if let Some(ref r) = risk_levels {
                        parse_risk_levels(r)?
                    } else {
                        vec![risk.clamp(1, 9)]
                    };
                    println!("{}", "=".repeat(80));
                    println!("🚀 BENCHMARK WITH OPTIMALS FROM ~/.rustrade");
                    println!("Symbols: {:?}", symbol_list);
                    println!("Periods: {} window(s)", period_list.len());
                    println!("Risk level(s): {:?}", risk_list);
                    println!("{}", "=".repeat(80));
                    for (period_label, start_p, end_p) in &period_list {
                        for score in &risk_list {
                            let params = persist
                                .get_for_risk_score(*score, asset_type)
                                .ok()
                                .flatten()
                                .context(format!(
                                    "No optimal params for risk {} in ~/.rustrade. Run: cargo run --bin optimize -- run --risk-score {}",
                                    score, score
                                ))?;
                            let config = optimal_params_to_analyst_config(&params, *score)?;
                            let strategy_label = format!("Optimal Risk-{}", score);
                            println!("\n📅 Period: {} Risk-{}", period_label, score);
                            for sym in &symbol_list {
                                match engine
                                    .run_single_with_config(sym, *start_p, *end_p, config.clone())
                                    .await
                                {
                                    Ok(res) => {
                                        results.push(convert_backtest_result(
                                            &res,
                                            sym,
                                            &strategy_label,
                                            period_label,
                                        ));
                                    }
                                    Err(e) => {
                                        println!("❌ Error for {} ({}): {}", sym, period_label, e);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Benchmark using best params from optimization JSON file
                    let content = std::fs::read_to_string(path)
                        .context(format!("Failed to read params file: {}", path))?;
                    let mut opt_results: Vec<OptimizationResult> = serde_json::from_str(&content)
                        .context(
                        "Failed to parse optimization JSON (expected array of OptimizationResult)",
                    )?;
                    if opt_results.is_empty() {
                        anyhow::bail!("Params file contains no results");
                    }
                    opt_results.sort_by(|a, b| {
                        b.objective_score
                            .partial_cmp(&a.objective_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let global_best = &opt_results[0];

                    // Best config for a given risk: use result with matching risk_score if any, else global best + apply_risk_appetite.
                    let best_config_for_risk = |score: u8| -> anyhow::Result<AnalystConfig> {
                        let with_score: Vec<&OptimizationResult> = opt_results
                            .iter()
                            .filter(|r| r.risk_score == Some(score))
                            .collect();
                        if with_score.is_empty() {
                            let mut cfg = global_best.params.clone();
                            let appetite = RiskAppetite::new(score)
                                .context("risk score in list should be 1-9")?;
                            cfg.apply_risk_appetite(&appetite);
                            Ok(cfg)
                        } else {
                            let best = with_score
                                .into_iter()
                                .max_by(|a, b| {
                                    a.objective_score
                                        .partial_cmp(&b.objective_score)
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                })
                                .unwrap();
                            Ok(best.params.clone())
                        }
                    };

                    let period_list: Vec<PeriodWindow> = if let Some(ref p) = periods {
                        parse_periods(p, is_crypto)?
                    } else {
                        vec![(
                            format!("{} to {}", start, end_dt.date_naive()),
                            start_dt,
                            end_dt,
                        )]
                    };

                    // When no --risk-levels, apply single --risk so optimized params are adapted to that risk.
                    let risk_list: Vec<u8> = if let Some(ref r) = risk_levels {
                        parse_risk_levels(r)?
                    } else {
                        vec![risk.clamp(1, 9)]
                    };

                    println!("{}", "=".repeat(80));
                    println!("🚀 BENCHMARK WITH OPTIMIZATION PARAMS");
                    println!("Params file: {}", path);
                    println!("Symbols: {:?}", symbol_list);
                    println!("Periods: {} window(s)", period_list.len());
                    println!("Risk level(s): {:?} (params adapted to risk)", risk_list);
                    println!(
                        "Config (global best): {:?} (fast={}, slow={}, rsi={:.0})",
                        global_best.params.strategy_mode,
                        global_best.params.fast_sma_period,
                        global_best.params.slow_sma_period,
                        global_best.params.rsi_threshold
                    );
                    println!("{}", "=".repeat(80));

                    let risks_to_run: Vec<Option<u8>> =
                        risk_list.into_iter().map(Some).collect::<Vec<_>>();

                    for (period_label, start_p, end_p) in &period_list {
                        for risk_opt in &risks_to_run {
                            let (config, strategy_label) = match risk_opt {
                                None => (
                                    global_best.params.clone(),
                                    format!("{:?}", global_best.params.strategy_mode),
                                ),
                                Some(score) => {
                                    let cfg = best_config_for_risk(*score)?;
                                    (
                                        cfg,
                                        format!(
                                            "{:?} Risk-{}",
                                            global_best.params.strategy_mode, score
                                        ),
                                    )
                                }
                            };
                            let risk_suffix =
                                risk_opt.map(|s| format!(" Risk-{}", s)).unwrap_or_default();
                            println!("\n📅 Period: {}{}", period_label, risk_suffix);
                            for sym in &symbol_list {
                                match engine
                                    .run_single_with_config(sym, *start_p, *end_p, config.clone())
                                    .await
                                {
                                    Ok(res) => {
                                        results.push(convert_backtest_result(
                                            &res,
                                            sym,
                                            &strategy_label,
                                            period_label,
                                        ));
                                    }
                                    Err(e) => {
                                        println!("❌ Error for {} ({}): {}", sym, period_label, e);
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                let strat_mode =
                    StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Standard);
                let run_risk_list: Vec<u8> = if let Some(ref r) = risk_levels {
                    parse_risk_levels(r)?
                } else {
                    vec![risk]
                };
                let period_label = format!("{} to {}", start, end_dt.date_naive());

                println!("{}", "=".repeat(80));
                println!("🚀 RUNNING BENCHMARK");
                println!("Symbols: {:?}", symbol_list);
                println!("Period: {} to {}", start_dt, end_dt);
                println!("Strategy: {:?}", strat_mode);
                println!("Risk level(s): {:?}", run_risk_list);
                println!("{}", "=".repeat(80));

                if parallel && symbol_list.len() > 1 && run_risk_list.len() == 1 {
                    let batch_results = engine
                        .run_parallel(symbol_list, start_dt, end_dt, strat_mode)
                        .await;
                    for _batch_res in batch_results {
                        // ...
                    }
                } else {
                    for risk_score in &run_risk_list {
                        let strategy_label = format!("{:?} Risk-{}", strat_mode, risk_score);
                        for sym in &symbol_list {
                            match engine
                                .run_single(sym, start_dt, end_dt, strat_mode, Some(*risk_score))
                                .await
                            {
                                Ok(res) => {
                                    results.push(convert_backtest_result(
                                        &res,
                                        sym,
                                        &strategy_label,
                                        &period_label,
                                    ));
                                }
                                Err(e) => {
                                    println!("❌ Error for {} (Risk-{}): {}", sym, risk_score, e)
                                }
                            }
                        }
                    }
                }
            }

            reporter.print_summary(&results);
            let report_label = params_file
                .as_ref()
                .map(|_| format!("Optimized {}", start))
                .unwrap_or_else(|| format!("Run {} {}", strategy, start));
            reporter.generate_report(&results, &report_label);
        }
        Commands::Matrix { symbol: _ } => {
            println!("🔬 RUNNING EXPANDED MATRIX BENCHMARK");

            // Define Symbols
            let symbols = vec!["TSLA", "NVDA", "AAPL", "AMD", "MSFT"];

            // Define Periods based on available data
            let periods = vec![
                (
                    "Dec 2024",
                    Utc.with_ymd_and_hms(2024, 12, 20, 14, 30, 0).unwrap(),
                    Utc.with_ymd_and_hms(2024, 12, 31, 21, 0, 0).unwrap(),
                ),
                (
                    "Jan 2025",
                    Utc.with_ymd_and_hms(2025, 1, 1, 14, 30, 0).unwrap(),
                    Utc.with_ymd_and_hms(2025, 1, 19, 21, 0, 0).unwrap(),
                ),
            ];

            let strategies = vec![
                StrategyMode::Standard,
                StrategyMode::Advanced,
                StrategyMode::Dynamic,
                StrategyMode::TrendRiding,
                StrategyMode::MeanReversion,
                StrategyMode::RegimeAdaptive,
                StrategyMode::SMC,
                StrategyMode::Momentum,
                StrategyMode::Breakout,
            ];
            // Testing Risk Sensitivity: Conservative (2), Neutral (5), Aggressive (8)
            let risks = vec![2, 5, 8];

            let mut results = Vec::new();

            for symbol in &symbols {
                println!("\n📦 SYMBOL: {}", symbol);
                for (period_name, start, end) in &periods {
                    println!(
                        "  📅 Period: {} ({} to {})",
                        period_name,
                        start.date_naive(),
                        end.date_naive()
                    );
                    for strat in &strategies {
                        for risk in &risks {
                            print!("    👉 {:?}... ", strat);
                            match engine
                                .run_single(symbol, *start, *end, *strat, Some(*risk))
                                .await
                            {
                                Ok(res) => {
                                    use rust_decimal::prelude::ToPrimitive;
                                    let ret_pct =
                                        res.total_return_pct.to_f64().unwrap_or(0.0) * 100.0;
                                    println!("Done. {:.2}%", ret_pct);

                                    let entry = convert_backtest_result(
                                        &res,
                                        symbol,
                                        &format!("{:?}", strat),
                                        period_name,
                                    );
                                    results.push(entry);
                                }
                                Err(e) => println!("Error: {}", e),
                            }
                        }
                    }
                }
            }
            reporter.print_summary(&results);
            reporter.generate_report(&results, "Matrix_Expanded");
        }
        Commands::Verify => {
            println!("✅ RUNNING VERIFICATION SUITE");
            let start = Utc.with_ymd_and_hms(2024, 1, 1, 14, 30, 0).unwrap();
            let end = Utc.with_ymd_and_hms(2024, 6, 30, 21, 0, 0).unwrap();
            let symbol = "NVDA";

            let scenarios = vec![
                (StrategyMode::Standard, 8, "Risk-8 Standard"),
                (StrategyMode::Breakout, 8, "Risk-8 Breakout"),
            ];

            let mut results = Vec::new();
            for (strat, risk, label) in scenarios {
                match engine
                    .run_single(symbol, start, end, strat, Some(risk))
                    .await
                {
                    Ok(res) => {
                        results.push(convert_backtest_result(&res, symbol, label, "2024 H1"));
                    }
                    Err(e) => println!("Error: {}", e),
                }
            }
            reporter.print_summary(&results);
            reporter.generate_report(&results, "Verification");
        }
    }

    Ok(())
}
