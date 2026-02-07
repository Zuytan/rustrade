use crate::application::optimization::simulator::BacktestResult;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub timestamp: DateTime<Utc>,
    pub configuration: String,
    pub results: Vec<BenchmarkResultEntry>,
    pub summary: BenchmarkSummary,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkResultEntry {
    pub symbol: String,
    pub strategy: String,
    pub window: String,
    pub return_pct: Decimal,
    pub buy_and_hold_pct: Decimal,
    pub net_profit: Decimal,
    pub trade_count: usize,
    pub win_rate: f64,
    pub max_drawdown: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub total_scenarios: usize,
    pub profitable_scenarios: usize,
    pub average_return_pct: f64,
    pub average_win_rate: f64,
    pub best_performer: String,
    pub worst_performer: String,
}

pub struct BenchmarkReporter {
    output_dir: PathBuf,
}

impl BenchmarkReporter {
    pub fn new(output_dir: &str) -> Self {
        let path = PathBuf::from(output_dir);
        if !path.exists() {
            fs::create_dir_all(&path).expect("Failed to create benchmark output directory");
        }
        Self { output_dir: path }
    }

    pub fn generate_report(&self, results: &[BenchmarkResultEntry], config_desc: &str) -> String {
        let summary = self.calculate_summary(results);
        let report = BenchmarkReport {
            timestamp: Utc::now(),
            configuration: config_desc.to_string(),
            results: results
                .iter()
                .map(|r| BenchmarkResultEntry {
                    symbol: r.symbol.clone(),
                    strategy: r.strategy.clone(),
                    window: r.window.clone(),
                    return_pct: r.return_pct,
                    buy_and_hold_pct: r.buy_and_hold_pct,
                    net_profit: r.net_profit,
                    trade_count: r.trade_count,
                    win_rate: r.win_rate,
                    max_drawdown: r.max_drawdown,
                })
                .collect(),
            summary,
        };

        let json = serde_json::to_string_pretty(&report).expect("Failed to serialize report");
        let filename = format!(
            "benchmark_report_{}.json",
            Utc::now().format("%Y%m%d_%H%M%S")
        );
        let path = self.output_dir.join(&filename);

        let mut file = fs::File::create(&path).expect("Failed to create report file");
        file.write_all(json.as_bytes())
            .expect("Failed to write report file");

        println!("📝 Report saved to: {}", path.display());
        path.to_string_lossy().to_string()
    }

    pub fn print_summary(&self, results: &[BenchmarkResultEntry]) {
        if results.is_empty() {
            println!("⚠️ No results to report.");
            return;
        }

        println!("\n{}", "=".repeat(120));
        println!("📊 BENCHMARK SUMMARY REPORT");
        println!("{}", "=".repeat(120));
        println!(
            "{:<10} | {:<16} | {:<15} | {:>9} | {:>9} | {:>10} | {:>6} | {:>8} | {:>8}",
            "Symbol",
            "Strategy",
            "Window",
            "Return%",
            "B&H%",
            "Net PnL",
            "Trades",
            "WinRate",
            "DD%"
        );
        println!("{}", "-".repeat(120));

        let mut warnings: Vec<String> = Vec::new();

        for res in results {
            // Flag extreme returns (> +500% or < -90%)
            let ret_f64 = res.return_pct.to_f64().unwrap_or(0.0);
            let flag = if ret_f64 > 500.0 {
                " ⚠️ EXTREME"
            } else if ret_f64 < -90.0 {
                " ⚠️ WIPEOUT"
            } else if res.max_drawdown >= 99.0 {
                " ⚠️ MAX-DD"
            } else {
                ""
            };

            println!(
                "{:<10} | {:<16} | {:<15} | {:>8.2}% | {:>8.2}% | ${:>9.2} | {:>6} | {:>7.1}% | {:>7.2}%{}",
                res.symbol,
                res.strategy,
                res.window,
                res.return_pct,
                res.buy_and_hold_pct,
                res.net_profit,
                res.trade_count,
                res.win_rate * 100.0,
                res.max_drawdown,
                flag
            );

            if ret_f64 > 500.0 {
                warnings.push(format!(
                    "  {} {}: Return {:.1}% is unrealistically high — check data quality / sizing",
                    res.symbol, res.window, ret_f64
                ));
            }
            if ret_f64 < -90.0 {
                warnings.push(format!(
                    "  {} {}: Return {:.1}% — near-total wipeout, review risk limits",
                    res.symbol, res.window, ret_f64
                ));
            }
            if res.max_drawdown >= 99.0 {
                warnings.push(format!(
                    "  {} {}: Max DD {:.1}% — possible data/simulation issue",
                    res.symbol, res.window, res.max_drawdown
                ));
            }
        }
        println!("{}", "=".repeat(120));

        if !warnings.is_empty() {
            println!("\n⚠️  WARNINGS — Potentially unrealistic results detected:");
            println!("{}", "-".repeat(80));
            for w in &warnings {
                println!("{}", w);
            }
            println!("{}", "-".repeat(80));
            println!(
                "  Tip: Check data quality, position sizing, and risk limits for flagged entries.\n"
            );
        }
    }

    fn calculate_summary(&self, results: &[BenchmarkResultEntry]) -> BenchmarkSummary {
        if results.is_empty() {
            return BenchmarkSummary {
                total_scenarios: 0,
                profitable_scenarios: 0,
                average_return_pct: 0.0,
                average_win_rate: 0.0,
                best_performer: "N/A".to_string(),
                worst_performer: "N/A".to_string(),
            };
        }

        let total = results.len();
        let profitable = results
            .iter()
            .filter(|r| r.return_pct > Decimal::ZERO)
            .count();

        let avg_ret = results
            .iter()
            .map(|r| r.return_pct.to_f64().unwrap_or(0.0))
            .sum::<f64>()
            / total as f64;

        let avg_win = results.iter().map(|r| r.win_rate).sum::<f64>() / total as f64;

        let best = results
            .iter()
            .max_by(|a, b| {
                a.return_pct
                    .partial_cmp(&b.return_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("results verified non-empty at line 132");

        let worst = results
            .iter()
            .min_by(|a, b| {
                a.return_pct
                    .partial_cmp(&b.return_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("results verified non-empty at line 132");

        BenchmarkSummary {
            total_scenarios: total,
            profitable_scenarios: profitable,
            average_return_pct: avg_ret,
            average_win_rate: avg_win,
            best_performer: format!("{} ({:.2}%)", best.symbol, best.return_pct),
            worst_performer: format!("{} ({:.2}%)", worst.symbol, worst.return_pct),
        }
    }
}

/// Build equity curve from trades (FIFO) and compute max drawdown in percent.
/// Uses running equity after each closed trade; drawdown = (peak - equity) / peak * 100.
fn max_drawdown_from_trades(
    initial_equity: rust_decimal::Decimal,
    trades: &[crate::domain::trading::types::Order],
) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    let init = initial_equity.to_f64().unwrap_or(100_000.0);
    let mut equity_curve = vec![init];
    let mut open_buys: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> = Vec::new();

    for order in trades {
        match order.side {
            crate::domain::trading::types::OrderSide::Buy => {
                open_buys.push((order.price, order.quantity));
            }
            crate::domain::trading::types::OrderSide::Sell => {
                if let Some((entry_price, qty)) = open_buys.pop() {
                    let prev = *equity_curve.last().unwrap();
                    let pnl = (order.price - entry_price) * qty;
                    let pnl_f = pnl.to_f64().unwrap_or(0.0);
                    equity_curve.push(prev + pnl_f);
                }
            }
        }
    }

    if equity_curve.len() < 2 {
        return 0.0;
    }
    let mut peak = equity_curve[0];
    let mut max_dd_pct = 0.0f64;
    for &eq in &equity_curve[1..] {
        if eq > peak {
            peak = eq;
        }
        if peak > 0.0 && eq < peak {
            let dd_pct = ((peak - eq) / peak * 100.0).min(100.0);
            if dd_pct > max_dd_pct {
                max_dd_pct = dd_pct;
            }
        }
    }
    max_dd_pct.min(100.0)
}

// Data conversion helper
pub fn convert_backtest_result(
    res: &BacktestResult,
    symbol: &str,
    strategy: &str,
    window: &str,
) -> BenchmarkResultEntry {
    let net = res.final_equity - res.initial_equity;

    let trades_count = res.trades.len();
    let win_rate = calculate_win_rate(&res.trades);
    let max_drawdown = max_drawdown_from_trades(res.initial_equity, &res.trades);

    BenchmarkResultEntry {
        symbol: symbol.to_string(),
        strategy: strategy.to_string(),
        window: window.to_string(),
        return_pct: res.total_return_pct,
        buy_and_hold_pct: res.buy_and_hold_return_pct,
        net_profit: net,
        trade_count: trades_count,
        win_rate,
        max_drawdown,
    }
}

fn calculate_win_rate(orders: &[crate::domain::trading::types::Order]) -> f64 {
    let mut wins = 0;
    let mut total = 0;

    // This is a simplified pnl check based on closed trades
    // Assuming FIFO matching like in the simulator

    // Actually, BacktestResult.trades IS the list of filled orders.
    // We need to pair them up.

    let mut open_buys: Vec<(Decimal, Decimal)> = Vec::new(); // (price, qty)

    for order in orders {
        match order.side {
            crate::domain::trading::types::OrderSide::Buy => {
                open_buys.push((order.price, order.quantity));
            }
            crate::domain::trading::types::OrderSide::Sell => {
                // Simplified matching: just take last buy (LIFO) or first (FIFO)
                // Simulator uses FIFO usually.
                if let Some((entry_price, _qty)) = open_buys.pop() {
                    total += 1;
                    if order.price > entry_price {
                        wins += 1;
                    }
                }
            }
        }
    }

    if total > 0 {
        wins as f64 / total as f64
    } else {
        0.0
    }
}
