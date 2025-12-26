use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rustrade::application::optimizer::{GridSearchOptimizer, ParameterGrid};
use rustrade::config::StrategyMode;
use rustrade::domain::portfolio::Portfolio;
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;
use rustrade::infrastructure::mock::MockExecutionService;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Setup Logging
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    // 2. Load Env
    dotenv::dotenv().ok();

    // 3. Parse Args
    let args: Vec<String> = env::args().collect();
    let mut symbol = "TSLA".to_string();
    let mut start_date_str = "2020-01-01".to_string();
    let mut end_date_str = "2023-12-31".to_string();
    let mut strategy_mode_str = "advanced".to_string();
    let mut grid_config_file: Option<String> = None;
    let mut output_file = "optimization_results.json".to_string();
    let mut top_n = 10;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--symbol" => {
                if i + 1 < args.len() {
                    symbol = args[i + 1].clone();
                    i += 1;
                }
            }
            "--start" => {
                if i + 1 < args.len() {
                    start_date_str = args[i + 1].clone();
                    i += 1;
                }
            }
            "--end" => {
                if i + 1 < args.len() {
                    end_date_str = args[i + 1].clone();
                    i += 1;
                }
            }
            "--strategy" => {
                if i + 1 < args.len() {
                    strategy_mode_str = args[i + 1].clone();
                    i += 1;
                }
            }
            "--grid-config" => {
                if i + 1 < args.len() {
                    grid_config_file = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--output" => {
                if i + 1 < args.len() {
                    output_file = args[i + 1].clone();
                    i += 1;
                }
            }
            "--top-n" => {
                if i + 1 < args.len() {
                    top_n = args[i + 1].parse().unwrap_or(10);
                    i += 1;
                }
            }
            "--help" => {
                print_help();
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    let strategy_mode = StrategyMode::from_str(&strategy_mode_str).unwrap_or(StrategyMode::Advanced);

    println!("{}", "=".repeat(80));
    println!("🔍 GRID SEARCH PARAMETER OPTIMIZER");
    println!("{}", "=".repeat(80));
    println!("Symbol:       {}", symbol);
    println!("Period:       {} to {}", start_date_str, end_date_str);
    println!("Strategy:     {:?}", strategy_mode);
    println!("Output:       {}", output_file);
    println!("{}", "=".repeat(80));

    // 4. Load parameter grid
    let parameter_grid = if let Some(config_file) = grid_config_file {
        info!("Loading parameter grid from: {}", config_file);
        load_grid_from_toml(&config_file)?
    } else {
        info!("Using default parameter grid");
        ParameterGrid::default()
    };

    println!("\n📊 Parameter Grid:");
    println!("  Fast SMA:       {:?}", parameter_grid.fast_sma);
    println!("  Slow SMA:       {:?}", parameter_grid.slow_sma);
    println!("  RSI Threshold:  {:?}", parameter_grid.rsi_threshold);
    println!("  Trend Div:      {:?}", parameter_grid.trend_divergence_threshold);
    println!("  ATR Mult:       {:?}", parameter_grid.trailing_stop_atr_multiplier);
    println!("  Cooldown (s):   {:?}", parameter_grid.order_cooldown_seconds);

    // Calculate total combinations
    let total_combos = parameter_grid.fast_sma.len()
        * parameter_grid.slow_sma.len()
        * parameter_grid.rsi_threshold.len()
        * parameter_grid.trend_divergence_threshold.len()
        * parameter_grid.trailing_stop_atr_multiplier.len()
        * parameter_grid.order_cooldown_seconds.len();

    println!("\n🔢 Total combinations to test: {}", total_combos);
    println!("{}\n", "=".repeat(80));

    // 5. Setup Dates
    let start_date_parsed = chrono::NaiveDate::parse_from_str(&start_date_str, "%Y-%m-%d")?;
    let start = Utc.from_utc_datetime(&start_date_parsed.and_hms_opt(14, 30, 0).unwrap());
    let end_date_parsed = chrono::NaiveDate::parse_from_str(&end_date_str, "%Y-%m-%d")?;
    let end = Utc.from_utc_datetime(&end_date_parsed.and_hms_opt(21, 0, 0).unwrap());

    // 6. Initialize Services
    let api_key = env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY must be set");
    let api_secret = env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY must be set");
    let ws_url =
        env::var("ALPACA_WS_URL").unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());
    let market_service = Arc::new(AlpacaMarketDataService::new(api_key, api_secret, ws_url));

    // Execution service factory - creates fresh portfolio for each run
    let execution_service_factory: Arc<dyn Fn() -> Arc<dyn rustrade::domain::ports::ExecutionService> + Send + Sync> =
        Arc::new(move || {
            let mut portfolio = Portfolio::new();
            portfolio.cash = Decimal::new(100000, 0);
            let portfolio_lock = Arc::new(RwLock::new(portfolio));

            let slippage_pct = env::var("SLIPPAGE_PCT")
                .unwrap_or_else(|_| "0.001".to_string())
                .parse::<f64>()
                .unwrap_or(0.001);
            let commission_per_share = env::var("COMMISSION_PER_SHARE")
                .unwrap_or_else(|_| "0.001".to_string())
                .parse::<f64>()
                .unwrap_or(0.001);

            Arc::new(MockExecutionService::with_costs(
                portfolio_lock,
                slippage_pct,
                commission_per_share,
            ))
        });

    // 7. Create optimizer
    let optimizer = GridSearchOptimizer::new(
        market_service,
        execution_service_factory,
        parameter_grid,
        strategy_mode,
    );

    // 8. Run optimization
    println!("🚀 Starting optimization...\n");
    let results = optimizer.run_optimization(&symbol, start, end).await?;

    // 9. Display top results
    println!("\n{}", "=".repeat(80));
    println!("✅ OPTIMIZATION COMPLETE - Top {} Results", top_n);
    println!("{}", "=".repeat(80));

    let top_results = optimizer.rank_results(results.clone(), top_n);

    println!(
        "{:<4} | {:<6} | {:<6} | {:>8} | {:>8} | {:>8} | {:>7} | {:>7} | {:>8}",
        "#", "Fast", "Slow", "Sharpe", "Return%", "WinRate", "Trades", "MaxDD%", "Score"
    );
    println!("{}", "-".repeat(80));

    for (i, result) in top_results.iter().enumerate() {
        println!(
            "{:<4} | {:<6} | {:<6} | {:>8.2} | {:>8.2} | {:>8.1} | {:>7} | {:>7.2} | {:>8.4}",
            i + 1,
            result.params.fast_sma_period,
            result.params.slow_sma_period,
            result.sharpe_ratio,
            result.total_return,
            result.win_rate,
            result.total_trades,
            result.max_drawdown,
            result.objective_score
        );
    }

    println!("{}\n", "=".repeat(80));

    // 10. Show best configuration details
    if let Some(best) = top_results.first() {
        println!("🏆 BEST CONFIGURATION:");
        println!("  Fast SMA:         {}", best.params.fast_sma_period);
        println!("  Slow SMA:         {}", best.params.slow_sma_period);
        println!("  RSI Threshold:    {:.1}", best.params.rsi_threshold);
        println!("  Trend Div:        {:.4}", best.params.trend_divergence_threshold);
        println!("  ATR Multiplier:   {:.1}", best.params.trailing_stop_atr_multiplier);
        println!("  Cooldown (s):     {}", best.params.order_cooldown_seconds);
        println!("\n  Sharpe Ratio:     {:.2}", best.sharpe_ratio);
        println!("  Total Return:     {:.2}%", best.total_return);
        println!("  Win Rate:         {:.1}%", best.win_rate);
        println!("  Max Drawdown:     {:.2}%", best.max_drawdown);
        println!("  Alpha:            {:.4}%", best.alpha * 100.0);
        println!("  Beta:             {:.2}", best.beta);
        println!("{}\n", "=".repeat(80));
    }

    // 11. Export to JSON
    let json_output = serde_json::to_string_pretty(&results)?;
    std::fs::write(&output_file, json_output)
        .context(format!("Failed to write results to {}", output_file))?;

    println!("💾 Results saved to: {}", output_file);
    println!("✅ Optimization complete!\n");

    Ok(())
}

fn load_grid_from_toml(path: &str) -> Result<ParameterGrid> {
    let content = std::fs::read_to_string(path)?;
    let grid: ParameterGrid = toml::from_str(&content)?;
    Ok(grid)
}

fn print_help() {
    println!("Grid Search Parameter Optimizer");
    println!("\nUSAGE:");
    println!("  cargo run --bin optimize -- [OPTIONS]");
    println!("\nOPTIONS:");
    println!("  --symbol <SYMBOL>           Symbol to optimize (default: TSLA)");
    println!("  --start <YYYY-MM-DD>        Start date (default: 2020-01-01)");
    println!("  --end <YYYY-MM-DD>          End date (default: 2023-12-31)");
    println!("  --strategy <MODE>           Strategy mode: standard, advanced, dynamic, trendriding, meanreversion");
    println!("  --grid-config <FILE>        TOML file with parameter grid (optional)");
    println!("  --output <FILE>             Output JSON file (default: optimization_results.json)");
    println!("  --top-n <N>                 Show top N results (default: 10)");
    println!("  --help                      Show this help message");
    println!("\nEXAMPLE:");
    println!("  cargo run --bin optimize -- \\");
    println!("    --symbol NVDA \\");
    println!("    --start 2020-01-01 \\");
    println!("    --end 2023-12-31 \\");
    println!("    --grid-config grid.toml \\");
    println!("    --output nvda_optimization.json");
}

