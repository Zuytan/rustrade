use chrono::{NaiveDate, TimeZone, Utc};
use clap::{Parser, Subcommand};
use rustrade::application::benchmarking::engine::BenchmarkEngine;
use rustrade::application::benchmarking::reporting::{BenchmarkReporter, convert_backtest_result};
use rustrade::config::StrategyMode;
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
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::ERROR.into())
                .add_directive("benchmark=info".parse().unwrap()),
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
        } => {
            let symbol_list: Vec<String> =
                symbols.split(',').map(|s| s.trim().to_string()).collect();
            let start_date = NaiveDate::parse_from_str(&start, "%Y-%m-%d")?;

            // Logic to determine dates
            let start_dt = Utc.from_utc_datetime(&start_date.and_hms_opt(14, 30, 0).unwrap());
            let end_dt = if let Some(e) = end {
                let end_date = NaiveDate::parse_from_str(&e, "%Y-%m-%d")?;
                Utc.from_utc_datetime(&end_date.and_hms_opt(21, 0, 0).unwrap())
            } else {
                start_dt + chrono::Duration::days(days)
            };

            let strat_mode = StrategyMode::from_str(&strategy).unwrap_or(StrategyMode::Standard);

            println!("{}", "=".repeat(80));
            println!("🚀 RUNNING BENCHMARK");
            println!("Symbols: {:?}", symbol_list);
            println!("Period: {} to {}", start_dt, end_dt);
            println!("Strategy: {:?}", strat_mode);
            println!("{}", "=".repeat(80));

            let mut results = Vec::new();

            if parallel && symbol_list.len() > 1 {
                let batch_results = engine
                    .run_parallel(symbol_list, start_dt, end_dt, strat_mode)
                    .await;
                for batch_res in batch_results {
                    match batch_res.result {
                        Ok(res) => {
                            results.push(convert_backtest_result(
                                &res,
                                &batch_res.symbol,
                                &strategy,
                                "Standard",
                            ));
                        }
                        Err(e) => {
                            println!("❌ Error for {}: {}", batch_res.symbol, e);
                        }
                    }
                }
            } else {
                for sym in symbol_list {
                    match engine
                        .run_single(&sym, start_dt, end_dt, strat_mode, None)
                        .await
                    {
                        Ok(res) => {
                            results
                                .push(convert_backtest_result(&res, &sym, &strategy, "Standard"));
                        }
                        Err(e) => println!("❌ Error for {}: {}", sym, e),
                    }
                }
            }

            reporter.print_summary(&results);
            reporter.generate_report(&results, &format!("Run {:?} {}", strat_mode, start));
        }
        Commands::Matrix { symbol } => {
            println!("🔬 RUNNING MATRIX BENCHMARK for {}", symbol);
            // Re-implementing simplified matrix logic
            // Fixed window: 2024 H1
            let start = Utc.with_ymd_and_hms(2024, 1, 1, 14, 30, 0).unwrap();
            let end = Utc.with_ymd_and_hms(2024, 6, 30, 21, 0, 0).unwrap();

            let strategies = vec![
                StrategyMode::Standard,
                StrategyMode::Breakout,
                StrategyMode::TrendRiding,
            ];
            let risks = vec![2, 5, 8];

            let mut results = Vec::new();

            for strat in strategies {
                for risk in &risks {
                    println!("Running {:?} Risk-{}...", strat, risk);
                    match engine
                        .run_single(&symbol, start, end, strat, Some(*risk))
                        .await
                    {
                        Ok(res) => {
                            let entry = convert_backtest_result(
                                &res,
                                &symbol,
                                &format!("{:?}", strat),
                                "2024 H1",
                            );
                            // Augment strategy name with risk
                            let mut augmented = entry;
                            augmented.strategy = format!("{:?} (R{})", strat, risk);
                            results.push(augmented);
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            reporter.print_summary(&results);
            reporter.generate_report(&results, "Matrix 2024 H1");
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
