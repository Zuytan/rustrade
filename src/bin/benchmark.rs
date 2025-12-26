use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rustrade::application::analyst::AnalystConfig;
use rustrade::application::simulator::Simulator;
use rustrade::config::StrategyMode;
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;
use rustrade::infrastructure::mock::MockExecutionService;
use rustrade::domain::portfolio::Portfolio;
use std::env;
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
    
    // 3. Parse Args (Simple manual parsing)
    let args: Vec<String> = env::args().collect();
    let mut symbol = "TSLA".to_string();
    let mut start_date_str = "2024-12-20".to_string(); // Default to a recent date
    let mut end_date_str = start_date_str.clone(); // Default to same day Close
    let mut strategy_mode_str = "standard".to_string();
    let mut batch_days: Option<i64> = None;

    // dumb parsing for now: --symbol X --start YYYY-MM-DD
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--symbol" => {
                if i + 1 < args.len() {
                    symbol = args[i+1].clone();
                    i += 1;
                }
            }
            "--start" => {
                if i + 1 < args.len() {
                    start_date_str = args[i+1].clone();
                    i += 1;
                }
            }
            "--end" => {
                if i + 1 < args.len() {
                    end_date_str = args[i+1].clone();
                    i += 1;
                }
            }
            "--strategy" => {
                if i + 1 < args.len() {
                    strategy_mode_str = args[i+1].clone();
                    i += 1;
                }
            }
            "--batch-days" => {
                if i + 1 < args.len() {
                    batch_days = Some(args[i+1].parse().expect("Invalid batch-days"));
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let strategy_mode = StrategyMode::from_str(&strategy_mode_str).unwrap_or(StrategyMode::Standard);

    println!("Starting Benchmark for Symbol: {} on Date: {} to Date: {} with Strategy: {:?}", symbol, start_date_str, end_date_str, strategy_mode);
    if let Some(days) = batch_days {
        println!("Batch Mode: {} day segments", days);
    }

    // 4. Setup Dates
    let start_date_parsed = chrono::NaiveDate::parse_from_str(&start_date_str, "%Y-%m-%d")?;
    let start = Utc.from_utc_datetime(&start_date_parsed.and_hms_opt(14, 30, 0).unwrap());
    let end_date_parsed = chrono::NaiveDate::parse_from_str(&end_date_str, "%Y-%m-%d")?;
    let final_end = Utc.from_utc_datetime(&end_date_parsed.and_hms_opt(21, 0, 0).unwrap());

    // 5. Initialize Services
    let api_key = env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY must be set");
    let api_secret = env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY must be set");
    let ws_url = env::var("ALPACA_WS_URL").unwrap_or("wss://stream.data.alpaca.markets/v2/iex".to_string());
    let market_service = Arc::new(AlpacaMarketDataService::new(api_key, api_secret, ws_url));
    
    let config = AnalystConfig {
        fast_sma_period: 20,
        slow_sma_period: 60,
        max_positions: 1,
        trade_quantity: Decimal::from(1),
        sma_threshold: 0.001,
        order_cooldown_seconds: 3600,
        risk_per_trade_percent: 0.02,
        strategy_mode: strategy_mode,
        trend_sma_period: 2000,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        trend_divergence_threshold: env::var("TREND_DIVERGENCE_THRESHOLD").unwrap_or("0.005".to_string()).parse().unwrap(),
        trailing_stop_atr_multiplier: env::var("TRAILING_STOP_ATR_MULTIPLIER").unwrap_or("3.0".to_string()).parse().unwrap(),
        atr_period: env::var("ATR_PERIOD").unwrap_or("14".to_string()).parse().unwrap(),
        rsi_threshold: env::var("RSI_THRESHOLD").unwrap_or("55.0".to_string()).parse().unwrap(),
            trend_riding_exit_buffer_pct: env::var("TREND_RIDING_EXIT_BUFFER_PCT").unwrap_or("0.03".to_string()).parse().unwrap(),
    };

    if let Some(days) = batch_days {
        // BATCH MODE
        let mut current_start = start;
        let mut batch_results = Vec::new();

        println!("\n{:<12} | {:<12} | {:<10} | {:<10} | {:<12} | {:<6}", 
            "Start", "End", "Return %", "B&H %", "Net Profit", "Trades");
        println!("{}", "-".repeat(75));

        while current_start < final_end {
            let mut current_end = current_start + chrono::Duration::days(days);
            if current_end > final_end {
                current_end = final_end;
            }
            // Ensure end time is market close
            current_end = current_end.date_naive().and_hms_opt(21, 0, 0).unwrap().and_utc();

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
                commission_per_share
            ));
            let simulator = Simulator::new(market_service.clone(), execution_service.clone(), config.clone());

            // Run
            match simulator.run(&symbol, current_start, current_end).await {
                Ok(result) => {
                    let net_profit = result.final_equity - result.initial_equity;
                    println!("{:<12} | {:<12} | {:>9.2}% | {:>9.2}% | ${:>11.2} | {:>6}", 
                        current_start.format("%Y-%m-%d"), 
                        current_end.format("%Y-%m-%d"), 
                        result.total_return_pct, 
                        result.buy_and_hold_return_pct, 
                        net_profit,
                        result.trades.len()
                    );
                    batch_results.push(result);
                },
                Err(e) => println!("Batch failed: {}", e),
            }

            // Move to next batch (start of next day)
            current_start = current_end + chrono::Duration::days(1); 
            current_start = current_start.date_naive().and_hms_opt(14, 30, 0).unwrap().and_utc();
        }

        // Aggregate Stats
        let total_batches = batch_results.len();
        
        if total_batches > 0 {
            let avg_return: f64 = batch_results.iter().map(|r| r.total_return_pct.to_f64().unwrap_or(0.0)).sum::<f64>() / total_batches as f64;
            let avg_bh_return: f64 = batch_results.iter().map(|r| r.buy_and_hold_return_pct.to_f64().unwrap_or(0.0)).sum::<f64>() / total_batches as f64;
            let positive_batches = batch_results.iter().filter(|r| r.total_return_pct > Decimal::ZERO).count();
            let total_trades: usize = batch_results.iter().map(|r| r.trades.len()).sum();

            println!("{}", "-".repeat(75));
            println!("SUMMARY ({} Batches)", total_batches);
            println!("Avg Return per Batch: {:.2}%", avg_return);
            println!("Avg Buy & Hold:       {:.2}%", avg_bh_return);
            println!("Win Rate (Positive):  {}/{} ({:.1}%)", positive_batches, total_batches, (positive_batches as f64 / total_batches as f64) * 100.0);
            println!("Total Trades:         {}", total_trades);
        } else {
            println!("{}", "-".repeat(75));
            println!("ERROR: No batches completed successfully!");
        }

    } else {
        // SINGLE RUN (Legacy)
        let end = final_end;
        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::new(100000, 0);
        let portfolio_lock = Arc::new(RwLock::new(portfolio));
        
        // Get transaction cost parameters  
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
            commission_per_share
        ));
    
        let simulator = Simulator::new(market_service.clone(), execution_service.clone(), config);
        let result = simulator.run(&symbol, start, end).await?;
        
        // Calculate period in days for annualization
        let period_days = (end - start).num_days() as f64;

        // Convert Orders to Trades by pairing Buy/Sell
        let mut trades: Vec<rustrade::domain::types::Trade> = Vec::new();
        let mut open_position: Option<&rustrade::domain::types::Order> = None;
        
        for order in &result.trades {
            match order.side {
                rustrade::domain::types::OrderSide::Buy => {
                    open_position = Some(order);
                }
                rustrade::domain::types::OrderSide::Sell => {
                    if let Some(buy_order) = open_position {
                        // Create a Trade from the pair
                        let pnl = (order.price - buy_order.price) * order.quantity;
                        trades.push(rustrade::domain::types::Trade {
                            id: order.id.clone(),
                            symbol: order.symbol.clone(),
                            side: rustrade::domain::types::OrderSide::Buy,
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

        // Calculate comprehensive metrics  
        let metrics = rustrade::domain::metrics::PerformanceMetrics::calculate(
            &trades,
            result.initial_equity,
            result.final_equity,
            period_days,
        );

        println!("--------------------------------------------------");
        println!("Benchmark Results for {}", symbol);
        println!("--------------------------------------------------");
        println!("Initial Equity: ${}", result.initial_equity.round_dp(2));
        println!("Final Equity:   ${}", result.final_equity.round_dp(2));
        println!("Total Return:   {:.2}%", result.total_return_pct);
        println!("Buy & Hold:     {:.2}%", result.buy_and_hold_return_pct);
        println!("--------------------------------------------------");
        println!("📊 PERFORMANCE METRICS");
        println!("--------------------------------------------------");
        println!("Sharpe Ratio:        {:.2}", metrics.sharpe_ratio);
        println!("Sortino Ratio:       {:.2}", metrics.sortino_ratio);
        println!("Calmar Ratio:        {:.2}", metrics.calmar_ratio);
        println!("Max Drawdown:        {:.2}%", metrics.max_drawdown_pct);
        println!("--------------------------------------------------");
        println!("📈 TRADE STATISTICS");
        println!("--------------------------------------------------");
        println!("Total Trades:        {}", metrics.total_trades);
        println!("Winning Trades:      {} ({:.1}%)", metrics.winning_trades, metrics.win_rate);
        println!("Losing Trades:       {}", metrics.losing_trades);
        println!("Profit Factor:       {:.2}", metrics.profit_factor);
        println!("Average Win:         ${:.2}", metrics.average_win);
        println!("Average Loss:        ${:.2}", metrics.average_loss);
        println!("Largest Win:         ${:.2}", metrics.largest_win);
        println!("Largest Loss:        ${:.2}", metrics.largest_loss);
        println!("Max Consecutive Wins: {}", metrics.max_consecutive_wins);
        println!("Max Consecutive Loss: {}", metrics.max_consecutive_losses);
        println!("--------------------------------------------------");
        println!("⏱️  EXPOSURE");
        println!("--------------------------------------------------");
        println!("Total Days:          {:.1}", metrics.total_days);
        println!("Days in Market:      {:.1}",  metrics.days_in_market);
        println!("Exposure:            {:.1}%", metrics.exposure_pct);
        println!("--------------------------------------------------");
        println!("TRADE HISTORY");
        println!("--------------------------------------------------");
        
        for (i, t) in result.trades.iter().enumerate() {
            let time = chrono::DateTime::from_timestamp_millis(t.timestamp).unwrap();
            println!("{}. [{}] {:?} {} shares @ ${}", 
                i+1, time, t.side, t.quantity, t.price);
        }
        println!("--------------------------------------------------");
    }

    Ok(())
}
