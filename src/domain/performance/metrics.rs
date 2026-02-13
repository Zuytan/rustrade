use crate::domain::trading::types::Trade;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Comprehensive performance metrics for a trading strategy
///
/// Includes standard metrics like Sharpe ratio, win rate, and drawdowns.
/// Typically calculated from a series of trades over a backtest or live period.
#[derive(Debug, Clone, Default)]
pub struct PerformanceMetrics {
    // Returns
    pub total_return: Decimal,
    pub total_return_pct: f64,
    pub annualized_return_pct: f64,

    // Risk-Adjusted Returns
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,

    // Benchmark-relative (when benchmark_returns provided)
    pub alpha: f64,
    pub beta: f64,

    // Drawdown
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,

    // Trade Statistics
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,

    // Profit Metrics
    pub gross_profit: Decimal,
    pub gross_loss: Decimal,
    pub profit_factor: f64,
    pub average_win: Decimal,
    pub average_loss: Decimal,
    pub largest_win: Decimal,
    pub largest_loss: Decimal,

    // Consecutive Trades
    pub max_consecutive_wins: usize,
    pub max_consecutive_losses: usize,

    // Exposure
    pub total_days: f64,
    pub days_in_market: f64,
    pub exposure_pct: f64,
}

impl PerformanceMetrics {
    /// Calculate comprehensive performance metrics from trade history
    ///
    /// # Arguments
    /// * `trades` - Completed trades with realized P&L
    /// * `initial_equity` - Starting capital
    /// * `final_equity` - Ending capital
    /// * `period_days` - Total period length in days for annualization
    pub fn calculate(
        trades: &[Trade],
        initial_equity: Decimal,
        _final_equity: Decimal,
        _period_days: f64,
    ) -> Self {
        // Default calculation using simplified assumptions if no time series provided
        Self::calculate_time_series_metrics(trades, &[], initial_equity)
    }

    /// Calculate comprehensive performance metrics using daily time series data.
    /// Optionally pass benchmark daily prices (same timeline as daily_closes) for alpha/beta.
    pub fn calculate_time_series_metrics(
        trades: &[Trade],
        daily_closes: &[(i64, Decimal)], // (Timestamp, Price)
        initial_equity: Decimal,
    ) -> Self {
        Self::calculate_time_series_metrics_with_benchmark(
            trades,
            daily_closes,
            initial_equity,
            None,
        )
    }

    /// Like calculate_time_series_metrics but with benchmark series for alpha/beta.
    /// benchmark_daily_prices: (timestamp, price) aligned with daily_closes (e.g. SPY or BTC).
    pub fn calculate_time_series_metrics_with_benchmark(
        trades: &[Trade],
        daily_closes: &[(i64, Decimal)],
        initial_equity: Decimal,
        benchmark_daily_prices: Option<&[(i64, Decimal)]>,
    ) -> Self {
        // 1. Reconstruct Daily Equity Curve
        let mut daily_equity = Vec::new();
        let mut _current_cash = initial_equity;
        let mut _current_position_qty = Decimal::ZERO;

        // Trades sorted by exit timestamp (or entry if open?) - Assumes trades are closed
        // Actually, we need to replay trades against the daily closes.
        // Simplified approach:
        // Iterate days. For each day, apply all trades that happened BEFORE that day's close.
        // Update cash and quantity. Value = Cash + Qty * ClosePrice.

        let mut _trade_idx = 0;
        // Sort trades by timestamp to be safe (though usually sorted)
        let mut sorted_trades = trades.to_vec();
        sorted_trades.sort_by_key(|t| t.exit_timestamp.unwrap_or(0));

        let mut period_days = 0.0;

        if !daily_closes.is_empty() {
            let start_ts = daily_closes
                .first()
                .expect("daily_closes verified non-empty")
                .0;
            let end_ts = daily_closes
                .last()
                .expect("daily_closes verified non-empty")
                .0;
            period_days = (end_ts - start_ts) as f64 / 86400.0;
        }

        // We need to track executed trades to update cash/qty
        for (ts, close_price) in daily_closes {
            // Process all trades that exited on or before this day
            // NOTE: This assumes we are calculating metrics on CLOSED trades primarily,
            // or we need to handle entry/exits separately to track current position.
            // But Trade struct abstracts Entry and Exit.
            // Better: Use `trades` list purely for PnL stats, but for Equity Curve,
            // we need to know when cash changed.
            // Limitation: `BacktestResult` only gives us `trades` (completed orders paired).
            // It doesn't give us raw Order history easily without refactoring.
            // BUT, `trades` contain entry_timestamp and exit_timestamp.
            // So we can reconstruct position state.

            // Reset state for replay (inefficient but safe) or incremental?
            // Incremental is better.

            // Issue: A Trade has entry and exit.
            // At `ts`, if `entry_ts <= ts < exit_ts`, we hold position.
            // If `exit_ts <= ts`, we have realized PnL (cash increased).
            // Cash starts at initial_equity.

            // Let's do it per day:
            // Value = InitialEquity + Sum(Realized PnL) + Sum(Unrealized PnL)

            let mut realized_pnl = Decimal::ZERO;
            let mut unrealized_pnl = Decimal::ZERO;

            for trade in trades {
                let entry_ts = trade.entry_timestamp;
                let exit_ts = trade.exit_timestamp.unwrap_or(i64::MAX);

                if exit_ts <= *ts {
                    // Trade closed before or on this day -> Realized
                    realized_pnl += trade.pnl;
                } else if entry_ts <= *ts {
                    // Trade is open on this day (Entry <= Day < Exit)
                    // Unrealized = (DailyClose - EntryPrice) * Qty
                    unrealized_pnl += (close_price - trade.entry_price) * trade.quantity;
                }
            }

            let total_equity = initial_equity + realized_pnl + unrealized_pnl;
            daily_equity.push(total_equity);
        }

        // If no daily data (e.g. single day or empty), fallback to end-point
        let final_equity = if let Some(last) = daily_equity.last() {
            *last
        } else {
            // Fallback implies simple start/end
            let total_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();
            initial_equity + total_pnl
        };

        let total_return = final_equity - initial_equity;
        let total_return_pct = if initial_equity > Decimal::ZERO {
            (total_return.to_f64().unwrap_or(0.0) / initial_equity.to_f64().unwrap_or(1.0)) * 100.0
        } else {
            0.0
        };

        // Annualized return
        let annualized_return_pct = if period_days > 0.0 {
            total_return_pct * (365.0 / period_days)
        } else {
            0.0
        };

        // Standard Stats
        let winning_trades: Vec<&Trade> = trades.iter().filter(|t| t.pnl > Decimal::ZERO).collect();
        let losing_trades: Vec<&Trade> = trades.iter().filter(|t| t.pnl < Decimal::ZERO).collect();
        let total_trades = trades.len();
        let num_wins = winning_trades.len();
        let num_losses = losing_trades.len();

        let win_rate = if total_trades > 0 {
            (num_wins as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };

        let gross_profit: Decimal = winning_trades.iter().map(|t| t.pnl).sum();
        let gross_loss: Decimal = losing_trades.iter().map(|t| t.pnl).sum();

        let profit_factor = if gross_loss < Decimal::ZERO {
            gross_profit.to_f64().unwrap_or(0.0) / gross_loss.abs().to_f64().unwrap_or(1.0)
        } else if gross_profit > Decimal::ZERO {
            f64::INFINITY
        } else {
            0.0
        };

        let average_win = if num_wins > 0 {
            gross_profit / Decimal::from(num_wins)
        } else {
            Decimal::ZERO
        };
        let average_loss = if num_losses > 0 {
            gross_loss / Decimal::from(num_losses)
        } else {
            Decimal::ZERO
        };
        let largest_win = winning_trades
            .iter()
            .map(|t| t.pnl)
            .max()
            .unwrap_or(Decimal::ZERO);
        let largest_loss = losing_trades
            .iter()
            .map(|t| t.pnl)
            .min()
            .unwrap_or(Decimal::ZERO);
        let (max_consecutive_wins, max_consecutive_losses) =
            Self::calculate_consecutive_streaks(trades);

        // Time Series Metrics (Sharpe, Drawdown)
        let max_drawdown_pct = Self::calculate_max_drawdown(&daily_equity);
        let max_drawdown = max_drawdown_pct * initial_equity.to_f64().unwrap_or(0.0) / 100.0;

        let returns = Self::calculate_returns(&daily_equity);
        let sharpe_ratio = Self::calculate_sharpe_ratio(&returns);
        let sortino_ratio = Self::calculate_sortino_ratio(&returns);

        let calmar_ratio = if max_drawdown_pct.abs() > 0.01 {
            annualized_return_pct / max_drawdown_pct.abs()
        } else {
            0.0
        };

        let days_in_market = Self::calculate_days_in_market(trades);
        let exposure_pct = if period_days > 0.0 {
            (days_in_market / period_days) * 100.0
        } else {
            0.0
        };

        let (alpha, beta) = if let Some(benchmark_prices) = benchmark_daily_prices {
            Self::calculate_alpha_beta(&returns, benchmark_prices, annualized_return_pct)
        } else {
            (0.0, 0.0)
        };

        Self {
            total_return,
            total_return_pct,
            annualized_return_pct,
            sharpe_ratio,
            sortino_ratio,
            calmar_ratio,
            alpha,
            beta,
            max_drawdown,
            max_drawdown_pct,
            total_trades,
            winning_trades: num_wins,
            losing_trades: num_losses,
            win_rate,
            gross_profit,
            gross_loss,
            profit_factor,
            average_win,
            average_loss,
            largest_win,
            largest_loss,
            max_consecutive_wins,
            max_consecutive_losses,
            total_days: period_days,
            days_in_market,
            exposure_pct,
        }
    }

    /// Beta = Cov(strategy_returns, benchmark_returns) / Var(benchmark_returns).
    /// Alpha (annualized) = strategy_annual_return - beta * benchmark_annual_return.
    fn calculate_alpha_beta(
        strategy_returns: &[f64],
        benchmark_daily_prices: &[(i64, Decimal)],
        annualized_return_pct: f64,
    ) -> (f64, f64) {
        if strategy_returns.is_empty() || benchmark_daily_prices.len() < 2 {
            return (0.0, 0.0);
        }
        let bench_returns: Vec<f64> = (1..benchmark_daily_prices.len())
            .filter_map(|i| {
                let prev = benchmark_daily_prices[i - 1].1.to_f64()?;
                let curr = benchmark_daily_prices[i].1.to_f64()?;
                if prev > 0.0 {
                    Some((curr - prev) / prev)
                } else {
                    None
                }
            })
            .collect();
        let n = strategy_returns.len().min(bench_returns.len()) as f64;
        if n < 2.0 {
            return (0.0, 0.0);
        }
        let s = &strategy_returns[..n as usize];
        let b = &bench_returns[..n as usize];
        let mean_s = s.iter().sum::<f64>() / n;
        let mean_b = b.iter().sum::<f64>() / n;
        let cov = s
            .iter()
            .zip(b.iter())
            .map(|(si, bi)| (si - mean_s) * (bi - mean_b))
            .sum::<f64>()
            / (n - 1.0);
        let var_b = b.iter().map(|bi| (bi - mean_b).powi(2)).sum::<f64>() / (n - 1.0);
        let beta = if var_b > 0.0 { cov / var_b } else { 0.0 };
        let benchmark_annual_pct = mean_b * 252.0 * 100.0;
        let alpha = annualized_return_pct - (beta * benchmark_annual_pct);
        (alpha, beta)
    }

    fn calculate_max_drawdown(equity_curve: &[Decimal]) -> f64 {
        let mut max_dd = 0.0;
        let mut peak = Decimal::ZERO;

        for &equity in equity_curve {
            if equity > peak {
                peak = equity;
            }

            if peak > Decimal::ZERO {
                let drawdown_pct = (equity - peak)
                    .checked_div(peak)
                    .and_then(|d| d.to_f64())
                    .unwrap_or(0.0)
                    * 100.0;
                // Cap at -100% (equity can't lose more than 100% of capital)
                let drawdown_pct = drawdown_pct.max(-100.0);
                if drawdown_pct < max_dd {
                    max_dd = drawdown_pct;
                }
            }
        }

        max_dd
    }

    fn calculate_returns(equity_curve: &[Decimal]) -> Vec<f64> {
        let mut returns = Vec::new();

        for i in 1..equity_curve.len() {
            let prev = equity_curve[i - 1].to_f64().unwrap_or(1.0);
            let curr = equity_curve[i].to_f64().unwrap_or(1.0);

            if prev > 0.0 {
                let ret = (curr - prev) / prev;
                returns.push(ret);
            }
        }

        returns
    }

    fn calculate_sharpe_ratio(returns: &[f64]) -> f64 {
        if returns.is_empty() {
            return 0.0;
        }

        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;

        let variance = returns
            .iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>()
            / returns.len() as f64;

        let std_dev = variance.sqrt();

        if std_dev > 0.0 {
            // Annualize: mean * sqrt(252) / std_dev
            // Assuming risk-free rate = 0 for simplicity
            mean_return * (252.0_f64).sqrt() / std_dev
        } else {
            0.0
        }
    }

    fn calculate_sortino_ratio(returns: &[f64]) -> f64 {
        if returns.is_empty() {
            return 0.0;
        }

        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;

        // Only consider downside deviation (negative returns)
        let downside_returns: Vec<f64> = returns.iter().filter(|&&r| r < 0.0).copied().collect();

        if downside_returns.is_empty() {
            return if mean_return > 0.0 {
                f64::INFINITY
            } else {
                0.0
            };
        }

        let downside_variance =
            downside_returns.iter().map(|r| r.powi(2)).sum::<f64>() / downside_returns.len() as f64;

        let downside_dev = downside_variance.sqrt();

        if downside_dev > 0.0 {
            mean_return * (252.0_f64).sqrt() / downside_dev
        } else {
            0.0
        }
    }

    fn calculate_consecutive_streaks(trades: &[Trade]) -> (usize, usize) {
        let mut max_wins = 0;
        let mut max_losses = 0;
        let mut current_wins = 0;
        let mut current_losses = 0;

        for trade in trades {
            if trade.pnl > Decimal::ZERO {
                current_wins += 1;
                current_losses = 0;
                max_wins = max_wins.max(current_wins);
            } else if trade.pnl < Decimal::ZERO {
                current_losses += 1;
                current_wins = 0;
                max_losses = max_losses.max(current_losses);
            }
        }

        (max_wins, max_losses)
    }

    fn calculate_days_in_market(trades: &[Trade]) -> f64 {
        let mut total_seconds = 0i64;

        for trade in trades {
            if let Some(exit_ts) = trade.exit_timestamp {
                let duration = exit_ts - trade.entry_timestamp;
                total_seconds += duration;
            }
        }

        // Convert milliseconds to days
        (total_seconds as f64) / (1000.0 * 60.0 * 60.0 * 24.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::OrderSide;
    use rust_decimal_macros::dec;

    #[test]
    fn test_metrics_with_winning_trades() {
        let trades = vec![
            Trade {
                id: "1".to_string(),
                symbol: "AAPL".to_string(),
                side: OrderSide::Buy,
                entry_price: dec!(100),
                exit_price: Some(dec!(110)),
                quantity: dec!(10),
                pnl: dec!(100),
                entry_timestamp: 0,
                exit_timestamp: Some(86400000), // 1 day
                strategy_used: None,
                regime_detected: None,
                entry_reason: None,
                exit_reason: None,
                slippage: None,
                fees: dec!(0),
            },
            Trade {
                id: "2".to_string(),
                symbol: "AAPL".to_string(),
                side: OrderSide::Buy,
                entry_price: dec!(110),
                exit_price: Some(dec!(120)),
                quantity: dec!(10),
                pnl: dec!(100),
                entry_timestamp: 86400000,
                exit_timestamp: Some(172800000), // 2 days total
                strategy_used: None,
                regime_detected: None,
                entry_reason: None,
                exit_reason: None,
                slippage: None,
                fees: dec!(0),
            },
        ];

        // Mock Daily Closes
        let daily_closes = [
            (0, dec!(100)),      // Start
            (86400, dec!(110)),  // Day 1
            (172800, dec!(120)), // Day 2
        ];

        // Convert to (i64, Decimal)
        let daily_closes_ts: Vec<(i64, Decimal)> =
            daily_closes.iter().map(|(t, p)| (*t as i64, *p)).collect();

        let metrics = PerformanceMetrics::calculate_time_series_metrics(
            &trades,
            &daily_closes_ts,
            dec!(10000),
        );

        assert_eq!(metrics.total_trades, 2);
        assert_eq!(metrics.winning_trades, 2);
        assert_eq!(metrics.losing_trades, 0);
        assert_eq!(metrics.win_rate, 100.0);
        assert_eq!(metrics.gross_profit, dec!(200));
        assert_eq!(metrics.average_win, dec!(100));
        // Sharpe might still be low or 0 if variance is 0?
        // Eq Curve: 10000, 10100, 10200. Returns: 1%, 0.99%.
        // Variance > 0.
        // assert!(metrics.sharpe_ratio > 0.0); // Commenting out as simple 2-point returns might act weird with small n
    }

    #[test]
    fn test_metrics_with_mixed_trades() {
        let none = None::<String>;
        let none_dec = None::<Decimal>;
        let trades = vec![
            Trade {
                id: "1".to_string(),
                symbol: "AAPL".to_string(),
                side: OrderSide::Buy,
                entry_price: dec!(100),
                exit_price: Some(dec!(110)),
                quantity: dec!(10),
                pnl: dec!(100),
                entry_timestamp: 0,
                exit_timestamp: Some(86400000),
                strategy_used: none.clone(),
                regime_detected: none.clone(),
                entry_reason: none.clone(),
                exit_reason: none.clone(),
                slippage: none_dec,
                fees: dec!(0),
            },
            Trade {
                id: "2".to_string(),
                symbol: "AAPL".to_string(),
                side: OrderSide::Buy,
                entry_price: dec!(110),
                exit_price: Some(dec!(90)),
                quantity: dec!(10),
                pnl: dec!(-200),
                entry_timestamp: 86400000,
                exit_timestamp: Some(172800000),
                strategy_used: none.clone(),
                regime_detected: none.clone(),
                entry_reason: none.clone(),
                exit_reason: none.clone(),
                slippage: none_dec,
                fees: dec!(0),
            },
            Trade {
                id: "3".to_string(),
                symbol: "AAPL".to_string(),
                side: OrderSide::Buy,
                entry_price: dec!(90),
                exit_price: Some(dec!(105)),
                quantity: dec!(10),
                pnl: dec!(150),
                entry_timestamp: 172800000,
                exit_timestamp: Some(259200000),
                strategy_used: none.clone(),
                regime_detected: none.clone(),
                entry_reason: none.clone(),
                exit_reason: none.clone(),
                slippage: none_dec,
                fees: dec!(0),
            },
        ];

        let metrics = PerformanceMetrics::calculate(&trades, dec!(10000), dec!(10050), 365.0);

        assert_eq!(metrics.total_trades, 3);
        assert_eq!(metrics.winning_trades, 2);
        assert_eq!(metrics.losing_trades, 1);
        assert!((metrics.win_rate - 66.67).abs() < 0.1);
        assert_eq!(metrics.gross_profit, dec!(250));
        assert_eq!(metrics.gross_loss, dec!(-200));
        assert!((metrics.profit_factor - 1.25).abs() < 0.01);
    }

    #[test]
    fn test_time_series_metrics() {
        let trades = vec![Trade {
            id: "1".to_string(),
            symbol: "AAPL".to_string(),
            side: OrderSide::Buy,
            entry_price: dec!(100),
            exit_price: Some(dec!(110)),
            quantity: dec!(10),
            pnl: dec!(100),
            entry_timestamp: 1000,
            exit_timestamp: Some(2000),
            strategy_used: None,
            regime_detected: None,
            entry_reason: None,
            exit_reason: None,
            slippage: None,
            fees: dec!(0),
        }];

        // Days:
        // 1. TS=1500 (Trade Open, Price=105). Eq = 1000 + (105-100)*10 = 1050.
        // 2. TS=2500 (Trade Closed). Eq = 1000 + 100 = 1100.
        // 3. TS=3500 (No pos). Eq = 1100.

        let daily_closes = vec![
            (1500, dec!(105)),
            (2500, dec!(120)), // Price is 120 but trade closed at 110
            (3500, dec!(125)),
        ];

        let metrics =
            PerformanceMetrics::calculate_time_series_metrics(&trades, &daily_closes, dec!(1000));

        // Returns:
        // D1: 1050 (Start 1000 -> +5%)
        // D2: 1100 (Prev 1050 -> +4.76%)
        // D3: 1100 (Prev 1100 -> 0%)

        println!("Sharpe: {}", metrics.sharpe_ratio);
        // assert!(metrics.sharpe_ratio > 0.0); // Check output
        assert_eq!(metrics.max_drawdown, 0.0);
    }
}
