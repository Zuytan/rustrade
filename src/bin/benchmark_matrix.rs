use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use rustrade::application::agents::analyst::AnalystConfig;
use rustrade::application::optimization::simulator::Simulator;
use rustrade::config::StrategyMode;
use rustrade::domain::risk::risk_appetite::RiskAppetite;
use rustrade::domain::trading::fee_model::ConstantFeeModel;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;
use rustrade::infrastructure::mock::MockExecutionService;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
struct MarketWindow {
    name: String,
    start: chrono::DateTime<Utc>,
    end: chrono::DateTime<Utc>,
}

#[derive(Debug)]
struct MatrixScenario {
    window: MarketWindow,
    symbol: String,
    strategy: StrategyMode,
    risk_score: u8,
}

#[derive(Debug)]
struct MatrixResult {
    scenario: MatrixScenario,
    return_pct: Decimal,
    buy_and_hold_pct: Decimal,
    net_profit: Decimal,
    trades_count: usize,
    win_rate: f64,
    drawdown: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup Logging (Minimal)
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::ERROR.into())
                .add_directive("benchmark_matrix=info".parse().unwrap()),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    // 2. Load Env
    if dotenv::from_filename(".env.benchmark").is_err() {
        dotenv::dotenv().ok();
    }

    // 3. Define Focused Market Windows
    let windows = vec![MarketWindow {
        name: "2024 H1 Surge".to_string(),
        start: Utc.with_ymd_and_hms(2024, 1, 1, 14, 30, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2024, 6, 30, 21, 0, 0).unwrap(),
    }];

    let symbols = vec!["NVDA", "AAPL"]; // Volatile vs Stable
    let strategies = vec![
        StrategyMode::Standard,    // FIXED target
        StrategyMode::Breakout,    // FIXED target
        StrategyMode::TrendRiding, // Control
    ];
    let risk_scores = vec![2, 8]; // Conservative vs Aggressive

    // 4. Initialize Shared Services
    let api_key = env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY must be set");
    let api_secret = env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY must be set");
    let data_url = env::var("ALPACA_DATA_URL").unwrap_or("https://data.alpaca.markets".to_string());
    let ws_url =
        env::var("ALPACA_WS_URL").unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());

    let base_config = rustrade::config::Config::from_env().unwrap_or_else(|_| {
        panic!("Failed to load ensure base config");
    });

    let market_service = Arc::new(
        AlpacaMarketDataService::builder()
            .api_key(api_key)
            .api_secret(api_secret)
            .data_base_url(data_url)
            .ws_url(ws_url)
            .min_volume_threshold(0.0)
            .asset_class(rustrade::config::AssetClass::Stock)
            .build(),
    );

    println!("{}", "=".repeat(160));
    println!("🚀 BENCHMARK MATRIX EXECUTION START (FOCUSED WINDOWS)");
    println!(
        "Windows: {:?}",
        windows.iter().map(|w| &w.name).collect::<Vec<_>>()
    );
    println!("Symbols: {:?}", symbols);
    println!("Strategies: {:?}", strategies);
    println!("Risk Scores: {:?}", risk_scores);
    println!("{}", "=".repeat(160));

    let mut results = Vec::new();
    let total_scenarios = windows.len() * symbols.len() * strategies.len() * risk_scores.len();
    let mut current_scenario = 0;

    for window in &windows {
        for symbol in &symbols {
            for strategy in &strategies {
                for risk_score in &risk_scores {
                    current_scenario += 1;

                    let scenario = MatrixScenario {
                        window: window.clone(),
                        symbol: symbol.to_string(),
                        strategy: *strategy,
                        risk_score: *risk_score,
                    };

                    print!(
                        "[{}/{}] Running Window '{}' Symbol {} Strategy {:?} Risk-{}... ",
                        current_scenario,
                        total_scenarios,
                        scenario.window.name,
                        scenario.symbol,
                        scenario.strategy,
                        scenario.risk_score
                    );
                    use std::io::Write;
                    std::io::stdout().flush().unwrap();

                    let mut app_config = base_config.clone();
                    app_config.strategy_mode = *strategy;
                    let risk_appetite = RiskAppetite::new(*risk_score).unwrap();
                    app_config.risk_appetite = Some(risk_appetite);

                    let mut config: AnalystConfig = app_config.into();
                    config.apply_risk_appetite(&risk_appetite);

                    match run_simulation(
                        symbol,
                        window.start,
                        window.end,
                        market_service.clone(),
                        config,
                    )
                    .await
                    {
                        Ok(res) => {
                            let net = res.final_equity - res.initial_equity;
                            let trades = convert_orders_to_trades(&res.trades);
                            let metrics = rustrade::domain::performance::metrics::PerformanceMetrics::calculate_time_series_metrics(
                                 &trades,
                                 &res.daily_closes,
                                 res.initial_equity,
                             );

                            let result_entry = MatrixResult {
                                scenario,
                                return_pct: res.total_return_pct,
                                buy_and_hold_pct: res.buy_and_hold_return_pct,
                                net_profit: net,
                                trades_count: trades.len(),
                                win_rate: metrics.win_rate,
                                drawdown: metrics.max_drawdown_pct,
                            };

                            println!(
                                "✅ {:.2}% (BH: {:.2}%)",
                                result_entry.return_pct, result_entry.buy_and_hold_pct
                            );
                            results.push(result_entry);
                        }
                        Err(e) => {
                            println!("❌ ERROR: {}", e);
                        }
                    }
                }
            }
        }
    }

    print_report(&results);

    Ok(())
}

async fn run_simulation(
    symbol: &str,
    start: chrono::DateTime<Utc>,
    end: chrono::DateTime<Utc>,
    market_service: Arc<AlpacaMarketDataService>,
    config: AnalystConfig,
) -> anyhow::Result<rustrade::application::optimization::simulator::BacktestResult> {
    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::new(100000, 0);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));

    let slippage = Decimal::from_f64(0.001).unwrap();
    let commission = Decimal::from_f64(0.001).unwrap();
    let fee_model = Arc::new(ConstantFeeModel::new(commission, slippage));

    let execution_service = Arc::new(MockExecutionService::with_costs(
        portfolio_lock.clone(),
        fee_model,
    ));

    let simulator = Simulator::new(market_service, execution_service, config);
    simulator.run(symbol, start, end).await
}

fn convert_orders_to_trades(
    orders: &[rustrade::domain::trading::types::Order],
) -> Vec<rustrade::domain::trading::types::Trade> {
    let mut trades = Vec::new();
    let mut open_position: Option<&rustrade::domain::trading::types::Order> = None;

    for order in orders {
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
    trades
}

fn print_report(results: &[MatrixResult]) {
    println!("\n\n");
    println!("{}", "=".repeat(160));
    println!("📊 BENCHMARK MATRIX REPORT");
    println!("{}", "=".repeat(160));
    println!(
        "{:<15} | {:<6} | {:<16} | {:<10} | {:>9} | {:>9} | {:>10} | {:>8} | {:>8} | {:>8}",
        "Window",
        "Symbol",
        "Strategy",
        "Risk",
        "Return%",
        "B&H%",
        "Net Profit",
        "Trades",
        "WinRate%",
        "Drawdown"
    );
    println!("{}", "-".repeat(160));

    for res in results {
        let risk_label = match res.scenario.risk_score {
            2 => "Cons(2)",
            5 => "Bal(5)",
            8 => "Aggr(8)",
            _ => "Unknown",
        };

        let strategy_name = format!("{:?}", res.scenario.strategy);
        let strategy_short = if strategy_name.len() > 16 {
            strategy_name[0..16].to_string()
        } else {
            strategy_name
        };

        println!(
            "{:<15} | {:<6} | {:<16} | {:<10} | {:>9.2} | {:>9.2} | ${:>9.0} | {:>8} | {:>7.1}% | {:>7.2}%",
            res.scenario.window.name,
            res.scenario.symbol,
            strategy_short,
            risk_label,
            res.return_pct,
            res.buy_and_hold_pct,
            res.net_profit,
            res.trades_count,
            res.win_rate,
            res.drawdown
        );
    }
    println!("{}", "=".repeat(160));
}
