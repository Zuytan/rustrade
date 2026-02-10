use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use clap::Parser;
use rust_decimal_macros::dec;
use rustrade::application::agents::analyst_config::AnalystConfig;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::config::StrategyMode;
use rustrade::domain::performance::metrics::PerformanceMetrics;
use rustrade::domain::ports::MarketDataService;
use rustrade::domain::risk::risk_appetite::RiskAppetite;
use rustrade::domain::trading::fee_model::ConstantFeeModel;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{Order, OrderSide, Trade};
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;
use rustrade::infrastructure::mock::MockExecutionService;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Symbol to optimize (default: BTC/USD)
    #[arg(short, long, default_value = "BTC/USD")]
    symbol: String,

    /// Lookback days (default: 30)
    #[arg(short, long, default_value = "30")]
    days: i64,

    /// Step size for weights (default: 0.1)
    #[arg(long, default_value = "0.1")]
    step: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    // Load env
    if dotenvy::from_filename(".env").is_err() {
        dotenvy::dotenv().ok();
    }

    let cli = Cli::parse();
    println!(
        "🚀 Starting Ensemble Optimization for {} ({} days)",
        cli.symbol, cli.days
    );

    // 1. Setup Market Data
    let api_key = env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY must be set");
    let api_secret = env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY must be set");
    let data_url = env::var("ALPACA_DATA_URL").unwrap_or("https://data.alpaca.markets".to_string());
    let api_base_url =
        env::var("ALPACA_BASE_URL").unwrap_or("https://paper-api.alpaca.markets".to_string());
    let ws_url =
        env::var("ALPACA_WS_URL").unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());

    let market_service = Arc::new(
        AlpacaMarketDataService::builder()
            .api_key(api_key)
            .api_secret(api_secret)
            .data_base_url(data_url.clone())
            .api_base_url(api_base_url)
            .ws_url(ws_url)
            .asset_class(rustrade::config::AssetClass::Crypto)
            .build(),
    );

    // 2. Fetch Data Once
    let end = Utc::now();
    let start = end - Duration::days(cli.days);

    println!("📥 Fetching historical data...");
    let bars = market_service
        .get_historical_bars(&cli.symbol, start, end, "1Min")
        .await
        .context("Failed to fetch historical bars")?;
    println!("✅ Fetched {} bars", bars.len());

    if bars.is_empty() {
        anyhow::bail!("No data found for symbol");
    }

    // Fetch SPY for alpha/beta (optional, but good for metrics)
    // For crypto benchmark, maybe BTC? But Simulator expects SPY usually.
    // We'll skip SPY for now or just fetch BTC as benchmark if symbol is not BTC.
    let benchmark_bars = if cli.symbol != "BTC/USD" {
        market_service
            .get_historical_bars("BTC/USD", start, end, "1Min")
            .await
            .ok()
    } else {
        None
    };

    // 3. Generate Grid
    let mut results = Vec::new();
    let step = cli.step;
    // Iterate w1 (Momentum)
    let steps = (1.0 / step).round() as i32;

    let mut count = 0;

    println!("🔄 Running simulations...");

    for i in 0..=steps {
        let w1 = (i as f64) * step;
        if w1 > 1.001 {
            continue;
        }

        for j in 0..=steps {
            let w2 = (j as f64) * step;
            if w1 + w2 > 1.001 {
                continue;
            }

            let w3 = 1.0 - w1 - w2;
            if w3 < -0.001 {
                continue;
            }
            let w3 = w3.max(0.0); // clean jitter

            // Weights: w1=Momentum, w2=ZScore, w3=SMC
            let mut weights = HashMap::new();
            weights.insert("StatMomentum".to_string(), w1);
            weights.insert("ZScoreMR".to_string(), w2);
            weights.insert("SMC".to_string(), w3);

            // Run Simulation
            let result = run_simulation(
                market_service.clone(),
                &bars,
                benchmark_bars.as_ref(),
                &cli.symbol,
                start,
                end,
                weights.clone(),
            )
            .await;

            match result {
                Ok(Some(m)) => {
                    results.push((w1, w2, w3, m));
                }
                Ok(None) => {
                    // No trades generated - this is expected for some weight combos
                    if count == 0 {
                        println!(
                            "⚠️  First simulation produced no trades. This may indicate a configuration issue."
                        );
                    }
                }
                Err(e) => {
                    eprintln!(
                        "❌ Simulation failed for weights ({:.2}, {:.2}, {:.2}): {}",
                        w1, w2, w3, e
                    );
                }
            }

            count += 1;
            if count % 10 == 0 {
                print!(".");
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
        }
    }
    println!("\n✅ Completed {} simulations", count);

    if results.is_empty() {
        println!("\n⚠️  WARNING: No valid configurations found!");
        println!("This likely means:");
        println!("  - Ensemble strategies are not generating enough signals");
        println!("  - Individual strategies (StatMomentum, ZScore, SMC) may need parameter tuning");
        println!("  - The voting threshold (0.5) may be too high for consensus");
        println!("\nTry:");
        println!("  1. Run with RUST_LOG=debug to see strategy voting details");
        println!("  2. Check if individual strategies work alone (change STRATEGY_MODE in .env)");
        println!("  3. Lower the consensus threshold in EnsembleStrategy");
        return Ok(());
    }

    // 4. Sort and Report
    // Sort by Sharpe Ratio
    results.sort_by(|a, b| b.3.sharpe_ratio.partial_cmp(&a.3.sharpe_ratio).unwrap());

    println!("\n🏆 TOP 5 CONFIGURATIONS:");
    println!(
        "{:<10} | {:<10} | {:<10} | {:<10} | {:<10} | {:<10}",
        "Momentum", "ZScore", "SMC", "Sharpe", "Return %", "Trades"
    );
    println!("{}", "-".repeat(70));

    for (w1, w2, w3, m) in results.iter().take(5) {
        println!(
            "{:<10.2} | {:<10.2} | {:<10.2} | {:<10.4} | {:<10.2} | {:<10}",
            w1, w2, w3, m.sharpe_ratio, m.total_return_pct, m.total_trades
        );
    }

    // Best params
    if let Some((best_w1, best_w2, best_w3, _)) = results.first() {
        println!("\n💡 Recommendation:");
        println!("Set weights in EnsembleStrategy:");
        println!("StatMomentum: {:.2}", best_w1);
        println!("ZScoreMR:     {:.2}", best_w2);
        println!("SMC:          {:.2}", best_w3);
    }

    Ok(())
}

async fn run_simulation(
    market_service: Arc<AlpacaMarketDataService>,
    bars: &[rustrade::domain::trading::types::Candle],
    benchmark_bars: Option<&Vec<rustrade::domain::trading::types::Candle>>, // Fixed type
    symbol: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    weights: HashMap<String, f64>,
) -> Result<Option<PerformanceMetrics>> {
    // Setup Portfolio
    let mut portfolio = Portfolio::new();
    portfolio.cash = dec!(100000);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));

    let fee_model = Arc::new(ConstantFeeModel::new(dec!(0.001), dec!(0.001)));
    let execution_service = Arc::new(MockExecutionService::with_costs(portfolio_lock, fee_model));

    // Config
    let mut config = AnalystConfig {
        strategy_mode: StrategyMode::Ensemble,
        ensemble_weights: Some(weights),
        ..Default::default()
    };
    // Ensure risk appetite is reasonable
    let appetite = RiskAppetite::new(5).unwrap();
    config.apply_risk_appetite(&appetite);

    // Simulator
    // Note: execution_service is Arc<MockExecutionService>, Simulator needs Arc<dyn ExecutionService>
    let simulator = Simulator::new(
        market_service,
        execution_service as Arc<dyn rustrade::domain::ports::ExecutionService>,
        config,
    );

    // Run with PRE-FETCHED bars
    // benchmark_bars needs to be cloned into a Vec? run_with_bars takes Option<Vec<Candle>>
    let spy_bars_clone = benchmark_bars.cloned();

    let result = simulator
        .run_with_bars(symbol, bars, start, end, spy_bars_clone)
        .await?;

    if result.trades.is_empty() {
        return Ok(None);
    }

    // Calculate Metrics
    let trades = orders_to_trades(&result.trades);
    let metrics = PerformanceMetrics::calculate_time_series_metrics(
        &trades,
        &result.daily_closes,
        result.initial_equity,
    );

    Ok(Some(metrics))
}

fn orders_to_trades(orders: &[Order]) -> Vec<Trade> {
    let mut trades = Vec::new();
    let mut open_position: Option<&Order> = None;
    for order in orders {
        match order.side {
            OrderSide::Buy => open_position = Some(order),
            OrderSide::Sell => {
                if let Some(buy) = open_position {
                    let pnl = (order.price - buy.price) * order.quantity;
                    trades.push(Trade {
                        id: order.id.clone(),
                        symbol: order.symbol.clone(),
                        side: OrderSide::Buy,
                        entry_price: buy.price,
                        exit_price: Some(order.price),
                        quantity: order.quantity,
                        pnl,
                        entry_timestamp: buy.timestamp,
                        exit_timestamp: Some(order.timestamp),
                        strategy_used: None,
                        regime_detected: None,
                        entry_reason: None,
                        exit_reason: None,
                        slippage: None,
                        fees: rust_decimal::Decimal::ZERO,
                    });
                    open_position = None;
                }
            }
        }
    }
    trades
}
