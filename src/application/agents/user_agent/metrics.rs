use super::UserAgent;
use crate::domain::trading::types::OrderSide;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

/// Direction of the market trend for a symbol
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrendDirection {
    Bullish,
    Bearish,
    Sideways,
}

impl TrendDirection {
    /// Returns an emoji representation of the trend
    pub fn emoji(&self) -> &'static str {
        match self {
            TrendDirection::Bullish => "📈",
            TrendDirection::Bearish => "📉",
            TrendDirection::Sideways => "→",
        }
    }
}

#[derive(Clone)]
pub struct StrategyInfo {
    pub mode: String,
    pub fast_sma: f64,
    pub slow_sma: f64,
    pub last_signal: Option<String>,
    pub trend: TrendDirection,
    pub current_price: Decimal,
}

impl UserAgent {
    /// Calculate SMAs and trend direction for a symbol
    pub fn calculate_trend(&self, symbol: &str) -> (f64, f64, TrendDirection) {
        let fast_period = 20;
        let slow_period = 50;

        let candles = match self.market_data.get(symbol) {
            Some(c) => c,
            None => return (0.0, 0.0, TrendDirection::Sideways),
        };

        // Calculate fast SMA
        let fast_sma = if candles.len() >= fast_period {
            let sum: f64 = candles[candles.len() - fast_period..]
                .iter()
                .map(|c| c.close.to_f64().unwrap_or(0.0))
                .sum();
            sum / fast_period as f64
        } else {
            0.0
        };

        // Calculate slow SMA
        let slow_sma = if candles.len() >= slow_period {
            let sum: f64 = candles[candles.len() - slow_period..]
                .iter()
                .map(|c| c.close.to_f64().unwrap_or(0.0))
                .sum();
            sum / slow_period as f64
        } else {
            0.0
        };

        // Determine trend based on SMA relationship
        let trend = if fast_sma == 0.0 || slow_sma == 0.0 {
            TrendDirection::Sideways
        } else {
            let diff_pct = (fast_sma - slow_sma) / slow_sma * 100.0;
            if diff_pct > 0.5 {
                TrendDirection::Bullish
            } else if diff_pct < -0.5 {
                TrendDirection::Bearish
            } else {
                TrendDirection::Sideways
            }
        };

        (fast_sma, slow_sma, trend)
    }

    /// Calculate total portfolio value (cash + positions)
    pub fn calculate_total_value(&self) -> Decimal {
        if let Ok(pf) = self.portfolio.try_read() {
            let mut position_value = Decimal::ZERO;
            for (symbol, pos) in &pf.positions {
                // Get current price from strategy_info
                let current_price = self
                    .strategy_info
                    .get(symbol)
                    .map(|info| info.current_price)
                    .unwrap_or(pos.average_price);
                position_value += pos.quantity * current_price;
            }
            pf.cash + position_value
        } else {
            Decimal::ZERO
        }
    }

    /// Calculate win rate as a percentage (0..100). Kept in Decimal until UI render.
    pub fn calculate_win_rate(&self) -> Decimal {
        if self.total_trades == 0 {
            Decimal::ZERO
        } else {
            Decimal::from(self.winning_trades) * dec!(100) / Decimal::from(self.total_trades)
        }
    }

    /// Calculate average win and loss percentages based on trade history.
    /// Returns (avg_win_pct, avg_loss_pct) as Decimal (e.g. 0.05 for 5%). Kept in Decimal until UI/simulation.
    pub fn calculate_trade_statistics(&self) -> (Decimal, Decimal) {
        if let Ok(pf) = self.portfolio.try_read() {
            let mut total_win_pct = Decimal::ZERO;
            let mut total_loss_pct = Decimal::ZERO;
            let mut win_count: u32 = 0;
            let mut loss_count: u32 = 0;

            for trade in &pf.trade_history {
                let exit_price = match trade.exit_price {
                    Some(p) => p,
                    None => continue,
                };
                if trade.entry_price.is_zero() {
                    continue;
                }

                // PnL percentage relative to entry price (Decimal throughout)
                // For LONG: (exit - entry) / entry; For SHORT: (entry - exit) / entry
                let pnl_per_unit = if trade.side == OrderSide::Buy {
                    exit_price - trade.entry_price
                } else {
                    trade.entry_price - exit_price
                };
                let pct = pnl_per_unit / trade.entry_price;

                if pct > Decimal::ZERO {
                    total_win_pct += pct;
                    win_count += 1;
                } else {
                    total_loss_pct += pct.abs();
                    loss_count += 1;
                }
            }

            let avg_win = if win_count > 0 {
                total_win_pct / Decimal::from(win_count)
            } else {
                Decimal::ZERO
            };
            let avg_loss = if loss_count > 0 {
                total_loss_pct / Decimal::from(loss_count)
            } else {
                Decimal::ZERO
            };

            // Safe defaults if no history yet (for Monte Carlo)
            let safe_win = if avg_win.is_zero() {
                dec!(0.02)
            } else {
                avg_win
            };
            let safe_loss = if avg_loss.is_zero() {
                dec!(0.015)
            } else {
                avg_loss
            };

            (safe_win, safe_loss)
        } else {
            (dec!(0.02), dec!(0.015))
        }
    }

    /// Calculate full performance metrics from portfolio history
    pub fn get_performance_metrics(
        &self,
    ) -> crate::domain::performance::metrics::PerformanceMetrics {
        if let Ok(pf) = self.portfolio.try_read() {
            // Use starting cash as initial equity
            let initial_equity = if pf.starting_cash > Decimal::ZERO {
                pf.starting_cash
            } else {
                dec!(10000) // Default fallback if not set
            };

            // Calculate period in days (from first trade to now, or 1 day min)
            let start_ts = pf
                .trade_history
                .first()
                .map(|t| t.entry_timestamp)
                .unwrap_or(chrono::Utc::now().timestamp_millis());
            let end_ts = chrono::Utc::now().timestamp_millis();
            let period_days = ((end_ts - start_ts) as f64 / (1000.0 * 3600.0 * 24.0)).max(1.0);

            crate::domain::performance::metrics::PerformanceMetrics::calculate(
                &pf.trade_history,
                initial_equity,
                self.calculate_total_value(), // Approximation with live prices via strategy_info
                period_days,
            )
        } else {
            crate::domain::performance::metrics::PerformanceMetrics::default()
        }
    }

    /// Generate equity curve points for plotting [timestamp, equity]
    pub fn get_equity_curve_points(&self) -> Vec<[f64; 2]> {
        if let Ok(pf) = self.portfolio.try_read() {
            let initial_equity = if pf.starting_cash > Decimal::ZERO {
                pf.starting_cash
            } else {
                dec!(10000)
            };

            let mut curve = Vec::new();
            // Start point
            let start_ts = pf
                .trade_history
                .first()
                .map(|t| t.entry_timestamp)
                .unwrap_or(chrono::Utc::now().timestamp_millis() - 86400000);

            curve.push([
                start_ts as f64 / 1000.0,
                initial_equity.to_f64().unwrap_or(0.0),
            ]);

            let mut current_equity = initial_equity;
            for trade in &pf.trade_history {
                current_equity += trade.pnl;
                if let Some(exit_ts) = trade.exit_timestamp {
                    curve.push([
                        exit_ts as f64 / 1000.0,
                        current_equity.to_f64().unwrap_or(0.0),
                    ]);
                }
            }

            // Add current point
            let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
            curve.push([now, current_equity.to_f64().unwrap_or(0.0)]);

            curve
        } else {
            vec![]
        }
    }
}
