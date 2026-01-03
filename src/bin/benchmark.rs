use chrono::{TimeZone, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rustrade::application::agents::analyst::AnalystConfig;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::config::StrategyMode;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;
use rustrade::infrastructure::mock::MockExecutionService;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup Logging
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    // 2. Load Env
    // Try loading .env.benchmark first, then fall back to default .env
    if dotenv::from_filename(".env.benchmark").is_err() {
        // If .env.benchmark doesn't exist, try standard .env
        dotenv::dotenv().ok();
    }

    // 3. Parse Args
    let args: Vec<String> = env::args().collect();
    let mut symbol = "TSLA".to_string();
    let mut start_date_str = "2024-12-20".to_string();
    let mut end_date_str = start_date_str.clone();
    let mut strategy_mode_str = "standard".to_string();
    let mut batch_days: Option<i64> = None;
    let mut dynamic_mode = false;
    let mut historical_scan_date: Option<chrono::NaiveDate> = None;
    let mut lookback_days = 30;

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
            "--batch-days" => {
                if i + 1 < args.len() {
                    batch_days = Some(args[i + 1].parse().expect("Invalid batch-days"));
                    i += 1;
                }
            }
            "--dynamic" => {
                dynamic_mode = true;
            }
            "--historical-scan" => {
                if i + 1 < args.len() {
                    let date_str = args[i + 1].clone();
                    historical_scan_date = Some(
                        chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                            .expect("Invalid date format YYYY-MM-DD"),
                    );
                    i += 1;
                }
            }
            "--days" => {
                if i + 1 < args.len() {
                    lookback_days = args[i + 1].parse().expect("Invalid days");
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let strategy_mode =
        StrategyMode::from_str(&strategy_mode_str).unwrap_or(StrategyMode::Standard);

    // 4. Initialize Services (Shared)
    let api_key = env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY must be set");
    let api_secret = env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY must be set");
    let ws_url =
        env::var("ALPACA_WS_URL").unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());
    let data_url = env::var("ALPACA_DATA_URL").unwrap_or("https://data.alpaca.markets".to_string());

    // Config for Min Volume
    let mut app_config = rustrade::config::Config::from_env().expect("Failed to load config");
    if strategy_mode != rustrade::config::StrategyMode::Standard {
        app_config.strategy_mode = strategy_mode;
    }
    let config: AnalystConfig = app_config.clone().into();

    println!("{}", "=".repeat(95));
    println!("⚙️  EFFECTIVE RISK CONFIGURATION");
    println!(
        "   Risk Per Trade:       {:.2}%",
        config.risk_per_trade_percent * 100.0
    );
    println!(
        "   Max Position Size:    {:.2}%",
        config.max_position_size_pct * 100.0
    );
    println!("   RSI Threshold:        {:.1}", config.rsi_threshold);
    println!(
        "   Trailing Stop:        {:.1}x ATR",
        config.trailing_stop_atr_multiplier
    );
    println!(
        "   Profit Target:        {:.1}x ATR",
        config.profit_target_multiplier
    );
    println!(
        "   Trend Tolerance:      {:.1}%",
        config.trend_tolerance_pct * 100.0
    );
    println!("   MACD Rising Required: {}", config.macd_requires_rising);
    println!("{}", "=".repeat(95));

    let market_service = Arc::new(AlpacaMarketDataService::new(
        api_key,
        api_secret,
        ws_url,
        data_url,
        app_config.min_volume_threshold, // Use configured threshold
        app_config.asset_class,          // Added
        None,                            // No caching needed for benchmarks
    ));

    // Determine target symbols and dates
    let targets: Vec<String>;
    let start: chrono::DateTime<Utc>;
    let final_end: chrono::DateTime<Utc>;

    if let Some(scan_date) = historical_scan_date {
        println!("{}", "=".repeat(95));
        println!("🕰️ HISTORICAL REPLAY MODE");
        println!(
            "Scanning Universe of {} symbols on {}...",
            BENCHMARK_UNIVERSE.len(),
            scan_date
        );

        // Convert static slice to Vec<String>
        let universe_vec: Vec<String> = BENCHMARK_UNIVERSE.iter().map(|&s| s.to_string()).collect();
        targets = market_service
            .get_historical_movers(scan_date, &universe_vec)
            .await?;

        if targets.is_empty() {
            println!("No movers found matching criteria on that date.");
            return Ok(());
        }

        println!("Selected Top Movers: {:?}", targets);
        println!("{}", "=".repeat(95));

        // Start benchmark from NEXT DAY after scan
        let start_naive = scan_date.succ_opt().unwrap();
        start = Utc.from_utc_datetime(&start_naive.and_hms_opt(14, 30, 0).unwrap());

        let end_naive = start_naive + chrono::Duration::days(lookback_days);
        final_end = Utc.from_utc_datetime(&end_naive.and_hms_opt(21, 0, 0).unwrap());

        println!(
            "Benchmarking Replay: {} to {} ({} days)",
            start, final_end, lookback_days
        );
    } else if dynamic_mode {
        println!("{}", "=".repeat(95));
        println!("🚀 DYNAMIC BENCHMARK MODE");
        println!("Fetching Top Movers using MarketDataService...");
        println!(
            "Min Volume Threshold: {:.0}",
            app_config.min_volume_threshold
        );

        use rustrade::domain::ports::MarketDataService;
        targets = market_service.get_top_movers().await?;
        println!("Found {} symbols: {:?}", targets.len(), targets);
        println!("{}", "=".repeat(95));

        if targets.is_empty() {
            println!("No symbols found to benchmark.");
            return Ok(());
        }

        // Dynamic dates: Last N days
        let now = Utc::now();
        final_end = now;
        start = now - chrono::Duration::days(lookback_days);

        println!(
            "Benchmarking Period: {} to {} ({} days)",
            start, final_end, lookback_days
        );
    } else {
        targets = vec![symbol];

        let start_date_parsed = chrono::NaiveDate::parse_from_str(&start_date_str, "%Y-%m-%d")?;
        start = Utc.from_utc_datetime(&start_date_parsed.and_hms_opt(14, 30, 0).unwrap());
        let end_date_parsed = chrono::NaiveDate::parse_from_str(&end_date_str, "%Y-%m-%d")?;
        final_end = Utc.from_utc_datetime(&end_date_parsed.and_hms_opt(21, 0, 0).unwrap());

        println!(
            "Starting Benchmark for Symbol: {} on Date: {} to Date: {} with Strategy: {:?}",
            targets[0], start_date_str, end_date_str, strategy_mode
        );
    }

    // Result Aggregation
    let mut all_results = Vec::new();

    for target_symbol in targets {
        println!("\n>> Benchmarking {}...", target_symbol);

        // Single Run Benchmarking Logic (Reused)
        // If batch_days is set, we do batch mode for THIS symbol.
        // If not, we do single run.

        if let Some(days) = batch_days {
            run_batch_benchmark(
                &target_symbol,
                start,
                final_end,
                days,
                market_service.clone(),
                config.clone(),
            )
            .await?;
        } else {
            let res = run_single_benchmark(
                &target_symbol,
                start,
                final_end,
                market_service.clone(),
                config.clone(),
            )
            .await?;
            // Print summary for this symbol
            println!(
                "   Return: {:.2}% | Net: ${:.2} | Trades: {}",
                res.total_return_pct,
                res.final_equity - res.initial_equity,
                res.trades.len()
            );
            all_results.push(res);
        }
    }

    if dynamic_mode && !all_results.is_empty() {
        println!("\n{}", "=".repeat(95));
        println!("📊 DYNAMIC PORTFOLIO SUMMARY");
        println!("{}", "=".repeat(95));

        let avg_return: f64 = all_results
            .iter()
            .map(|r| r.total_return_pct.to_f64().unwrap_or(0.0))
            .sum::<f64>()
            / all_results.len() as f64;
        let avg_bh: f64 = all_results
            .iter()
            .map(|r| r.buy_and_hold_return_pct.to_f64().unwrap_or(0.0))
            .sum::<f64>()
            / all_results.len() as f64;

        println!("Average Algorithmic Return: {:.2}%", avg_return);
        println!("Average Buy & Hold Return:  {:.2}%", avg_bh);
        println!("Outperformance:             {:.2}%", avg_return - avg_bh);
        println!("{}", "=".repeat(95));

        // List details
        println!(
            "{:<10} | {:>9} | {:>9} | {:>13}",
            "Symbol", "Return%", "B&H%", "Net Profit"
        );
        println!("{}", "-".repeat(50));
        for res in &all_results {
            // BacktestResult does not have symbol. We can't print it easily without changing struct or passing it.
            // We will just print the loop index or skip symbol column for now or infer it if possible.
            // For now, simple summary.
            let net = res.final_equity - res.initial_equity;
            println!(
                "{:<10} | {:>8.2}% | {:>8.2}% | ${:>12.2}",
                "---", // Placeholder
                res.total_return_pct,
                res.buy_and_hold_return_pct,
                net
            );
        }
    }

    Ok(())
}

// --- Constants ---
const BENCHMARK_UNIVERSE: &[&str] = &[
    "AAPL", "MSFT", "GOOGL", "AMZN", "NVDA", "TSLA", "META", "BRK.B", "LLY", "V", "JPM", "XOM",
    "WMT", "UNH", "MA", "PG", "JNJ", "HD", "MRK", "COST", "ABBV", "CVX", "CRM", "BAC", "AMD",
    "NFLX", "KO", "PEP", "ADBE", "TMO", "DIS", "WFC", "CSCO", "MCD", "ABT", "CAT", "INTC", "CMCSA",
    "PFE", "VZ", "UBER", "INTU", "AMAT", "IBM", "AMGN", "NOW", "TXN", "SPGI", "GE", "UNP",
];

// Helper Functions

async fn run_single_benchmark(
    symbol: &str,
    start: chrono::DateTime<Utc>,
    end: chrono::DateTime<Utc>,
    market_service: Arc<AlpacaMarketDataService>,
    config: AnalystConfig,
) -> Result<
    rustrade::application::optimization::simulator::BacktestResult,
    Box<dyn std::error::Error>,
> {
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

    let execution_service = Arc::new(MockExecutionService::with_costs(
        portfolio_lock.clone(),
        slippage_pct,
        commission_per_share,
    ));

    let simulator = Simulator::new(market_service.clone(), execution_service.clone(), config);
    let result = simulator.run(symbol, start, end).await?;
    Ok(result)
}

async fn run_batch_benchmark(
    symbol: &str,
    start: chrono::DateTime<Utc>,
    final_end: chrono::DateTime<Utc>,
    days: i64,
    market_service: Arc<AlpacaMarketDataService>,
    config: AnalystConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let batch_days_val = days;
    println!("{}", "=".repeat(95));
    println!(
        "🚀 BATCH BENCHMARK MODE - {} ({} Day Segments)",
        symbol, batch_days_val
    );
    println!("{}", "=".repeat(115));
    println!(
        "{:<4} | {:<12} | {:<12} | {:>9} | {:>9} | {:>13} | {:>6} | {:>8} | {:>6} | {:>6} | {:<6}",
        "#",
        "Start",
        "End",
        "Return%",
        "B&H%",
        "Net Profit",
        "Trades",
        "Sharpe",
        "Alpha",
        "Beta",
        "Status"
    );
    println!("{}", "-".repeat(115));

    let mut batch_results = Vec::new();
    let mut current_start = start;
    let mut batch_num = 1;
    let total_batches_estimate = ((final_end - start).num_days() / batch_days_val).max(1);

    while current_start < final_end {
        let mut current_end = current_start + chrono::Duration::days(days);
        if current_end > final_end {
            current_end = final_end;
        }
        // Ensure end time is market close
        current_end = current_end
            .date_naive()
            .and_hms_opt(21, 0, 0)
            .unwrap()
            .and_utc();

        // Setup FRESH Simulation environment for each batch
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0); // Reset cash
        let portfolio_lock = Arc::new(RwLock::new(portfolio));

        // Get transaction cost parameters from env
        let slippage_pct = env::var("SLIPPAGE_PCT")
            .unwrap_or_else(|_| "0.001".to_string())
            .parse::<f64>()
            .unwrap_or(0.001);
        let commission_per_share = env::var("COMMISSION_PER_SHARE")
            .unwrap_or_else(|_| "0.001".to_string())
            .parse::<f64>()
            .unwrap_or(0.001);

        let execution_service = Arc::new(MockExecutionService::with_costs(
            portfolio_lock.clone(),
            slippage_pct,
            commission_per_share,
        ));
        let simulator = Simulator::new(
            market_service.clone(),
            execution_service.clone(),
            config.clone(),
        );

        // Run
        match simulator.run(symbol, current_start, current_end).await {
            Ok(result) => {
                let net_profit = result.final_equity - result.initial_equity;

                // Convert Orders to Trades for metrics
                let mut trades: Vec<rustrade::domain::trading::types::Trade> = Vec::new();
                let mut open_position: Option<&rustrade::domain::trading::types::Order> = None;

                for order in &result.trades {
                    match order.side {
                        rustrade::domain::trading::types::OrderSide::Buy => {
                            open_position = Some(order);
                        }
                        rustrade::domain::trading::types::OrderSide::Sell => {
                            if let Some(buy_order) = open_position {
                                let pnl = (order.price - buy_order.price) * order.quantity;
                                trades.push(rustrade::domain::trading::types::Trade {
                                    id: order.id.clone(),
                                    symbol: order.symbol.clone(),
                                    side: rustrade::domain::trading::types::OrderSide::Buy,
                                    entry_price: buy_order.price,
                                    exit_price: Some(order.price),
                                    quantity: order.quantity,
                                    pnl,
                                    entry_timestamp: buy_order.timestamp,
                                    exit_timestamp: Some(order.timestamp),
                                });
                                open_position = None;
                            }
                        }
                    }
                }

                // Calculate accurate metrics for this batch
                let metrics = rustrade::domain::performance::metrics::PerformanceMetrics::calculate_time_series_metrics(
                        &trades,
                        &result.daily_closes,
                        result.initial_equity,
                    );
                let sharpe = metrics.sharpe_ratio;

                // Status indicator
                let status = if result.total_return_pct > Decimal::ZERO {
                    "✅ WIN"
                } else if result.total_return_pct < Decimal::ZERO {
                    "❌ LOSS"
                } else {
                    "➖ FLAT"
                };

                // Progress indicator
                let progress = format!("({}/~{})", batch_num, total_batches_estimate);

                println!(
                        "{:<4} | {:<12} | {:<12} | {:>9.2}% | {:>9.2}% | ${:>11.2} | {:>6} | {:>8.2} | {:>6.2} | {:>6.2} | {}",
                        progress,
                        current_start.format("%Y-%m-%d"),
                        current_end.format("%Y-%m-%d"),
                        result.total_return_pct,
                        result.buy_and_hold_return_pct,
                        net_profit,
                        result.trades.len(),
                        sharpe,
                        result.alpha * 100.0, // Convert to percentage
                        result.beta,
                        status
                    );
                batch_results.push(result);
                batch_num += 1;
            }
            Err(e) => {
                println!(
                    "{:<4} | {:<12} | {:<12} | ERROR: {}",
                    format!("({}/~{})", batch_num, total_batches_estimate),
                    current_start.format("%Y-%m-%d"),
                    current_end.format("%Y-%m-%d"),
                    e
                );
                batch_num += 1;
            }
        }

        // Move to next batch (start of next day)
        current_start = current_end + chrono::Duration::days(1);
        current_start = current_start
            .date_naive()
            .and_hms_opt(14, 30, 0)
            .unwrap()
            .and_utc();
    }

    // Aggregate Stats
    let total_batches = batch_results.len();

    if total_batches > 0 {
        let avg_return: f64 = batch_results
            .iter()
            .map(|r| r.total_return_pct.to_f64().unwrap_or(0.0))
            .sum::<f64>()
            / total_batches as f64;
        let avg_bh_return: f64 = batch_results
            .iter()
            .map(|r| r.buy_and_hold_return_pct.to_f64().unwrap_or(0.0))
            .sum::<f64>()
            / total_batches as f64;
        let positive_batches = batch_results
            .iter()
            .filter(|r| r.total_return_pct > Decimal::ZERO)
            .count();
        let negative_batches = batch_results
            .iter()
            .filter(|r| r.total_return_pct < Decimal::ZERO)
            .count();
        let total_trades: usize = batch_results.iter().map(|r| r.trades.len()).sum();

        // Calculate average Return across batches (Sharpe not available in BacktestResult)
        let avg_sharpe = 0.0; // Placeholder - would need full metrics implementation

        // Best and worst batch
        let best_batch = batch_results
            .iter()
            .max_by(|a, b| a.total_return_pct.partial_cmp(&b.total_return_pct).unwrap())
            .unwrap();
        let worst_batch = batch_results
            .iter()
            .min_by(|a, b| a.total_return_pct.partial_cmp(&b.total_return_pct).unwrap())
            .unwrap();

        println!("{}", "=".repeat(95));
        println!("📊 BATCH SUMMARY - {} Batches Completed", total_batches);
        println!("{}", "=".repeat(95));
        println!("Average Return:       {:.2}%", avg_return);
        println!("Average Buy & Hold:   {:.2}%", avg_bh_return);
        println!("Outperformance:       {:.2}%", avg_return - avg_bh_return);
        println!("{}", "-".repeat(95));
        println!(
            "Win Rate:             {}/{} ({:.1}%) ✅",
            positive_batches,
            total_batches,
            (positive_batches as f64 / total_batches as f64) * 100.0
        );
        println!(
            "Loss Rate:            {}/{} ({:.1}%) ❌",
            negative_batches,
            total_batches,
            (negative_batches as f64 / total_batches as f64) * 100.0
        );
        println!("Total Trades:         {}", total_trades);
        println!(
            "Trades per Batch:     {:.1}",
            total_trades as f64 / total_batches as f64
        );
        println!("{}", "-".repeat(95));
        println!("Average Sharpe Ratio: {:.2}", avg_sharpe);
        println!("Best Batch Return:    {:.2}%", best_batch.total_return_pct);
        println!("Worst Batch Return:   {:.2}%", worst_batch.total_return_pct);
        println!("{}", "=".repeat(95));
    } else {
        println!("{}", "=".repeat(95));
        println!("❌ ERROR: No batches completed successfully!");
        println!("{}", "=".repeat(95));
    }
    Ok(())
}
