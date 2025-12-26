use crate::domain::types::Trade;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Comprehensive performance metrics for trading strategies
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    // Returns
    pub total_return: Decimal,
    pub total_return_pct: f64,
    pub annualized_return_pct: f64,
    
    // Risk-Adjusted Returns
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,
    
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
        final_equity: Decimal,
        period_days: f64,
    ) -> Self {
        let total_return = final_equity - initial_equity;
        let total_return_pct = if initial_equity > Decimal::ZERO {
            (total_return.to_f64().unwrap_or(0.0) / initial_equity.to_f64().unwrap_or(1.0)) * 100.0
        } else {
            0.0
        };
        
        // Annualized return (simple, not compounded)
        let annualized_return_pct = if period_days > 0.0 {
            total_return_pct * (365.0 / period_days)
        } else {
            0.0
        };
        
        // Separate winning and losing trades
        let winning_trades: Vec<&Trade> = trades.iter()
            .filter(|t| t.pnl > Decimal::ZERO)
            .collect();
        let losing_trades: Vec<&Trade> = trades.iter()
            .filter(|t| t.pnl < Decimal::ZERO)
            .collect();
        
        let total_trades = trades.len();
        let num_wins = winning_trades.len();
        let num_losses = losing_trades.len();
        
        let win_rate = if total_trades > 0 {
            (num_wins as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };
        
        // Profit calculations
        let gross_profit: Decimal = winning_trades.iter().map(|t| t.pnl).sum();
        let gross_loss: Decimal = losing_trades.iter().map(|t| t.pnl).sum();
        
        let profit_factor = if gross_loss < Decimal::ZERO {
            gross_profit.to_f64().unwrap_or(0.0) / gross_loss.abs().to_f64().unwrap_or(1.0)
        } else {
            if gross_profit > Decimal::ZERO { f64::INFINITY } else { 0.0 }
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
        
        let largest_win = winning_trades.iter()
            .map(|t| t.pnl)
            .max()
            .unwrap_or(Decimal::ZERO);
        
        let largest_loss = losing_trades.iter()
            .map(|t| t.pnl)
            .min()
            .unwrap_or(Decimal::ZERO);
        
        // Consecutive wins/losses
        let (max_consecutive_wins, max_consecutive_losses) = Self::calculate_consecutive_streaks(trades);
        
        // Calculate equity curve for drawdown and Sharpe
        let equity_curve = Self::build_equity_curve(trades, initial_equity);
        let max_drawdown_pct = Self::calculate_max_drawdown(&equity_curve);
        let max_drawdown = max_drawdown_pct * initial_equity.to_f64().unwrap_or(0.0) / 100.0;
        
        // Calculate returns for Sharpe/Sortino
        let returns = Self::calculate_returns(&equity_curve);
        let sharpe_ratio = Self::calculate_sharpe_ratio(&returns);
        let sortino_ratio = Self::calculate_sortino_ratio(&returns);
        
        // Calmar Ratio = Annualized Return / Max Drawdown
        let calmar_ratio = if max_drawdown_pct.abs() > 0.01 {
            annualized_return_pct / max_drawdown_pct.abs()
        } else {
            0.0
        };
        
        // Exposure calculation (days in market)
        let days_in_market = Self::calculate_days_in_market(trades);
        let exposure_pct = if period_days > 0.0 {
            (days_in_market / period_days) * 100.0
        } else {
            0.0
        };
        
        Self {
            total_return,
            total_return_pct,
            annualized_return_pct,
            sharpe_ratio,
            sortino_ratio,
            calmar_ratio,
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
    
    fn build_equity_curve(trades: &[Trade], initial_equity: Decimal) -> Vec<Decimal> {
        let mut curve = vec![initial_equity];
        let mut current_equity = initial_equity;
        
        for trade in trades {
            current_equity += trade.pnl;
            curve.push(current_equity);
        }
        
        curve
    }
    
    fn calculate_max_drawdown(equity_curve: &[Decimal]) -> f64 {
        let mut max_dd = 0.0;
        let mut peak = Decimal::ZERO;
        
        for &equity in equity_curve {
            if equity > peak {
                peak = equity;
            }
            
            if peak > Decimal::ZERO {
                let drawdown_pct = ((equity - peak) / peak).to_f64().unwrap_or(0.0) * 100.0;
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
        
        let variance = returns.iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>() / returns.len() as f64;
        
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
        let downside_returns: Vec<f64> = returns.iter()
            .filter(|&&r| r < 0.0)
            .copied()
            .collect();
        
        if downside_returns.is_empty() {
            return if mean_return > 0.0 { f64::INFINITY } else { 0.0 };
        }
        
        let downside_variance = downside_returns.iter()
            .map(|r| r.powi(2))
            .sum::<f64>() / downside_returns.len() as f64;
        
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
    use crate::domain::types::OrderSide;
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
            },
        ];
        
        let metrics = PerformanceMetrics::calculate(&trades, dec!(10000), dec!(10200), 365.0);
        
        assert_eq!(metrics.total_trades, 2);
        assert_eq!(metrics.winning_trades, 2);
        assert_eq!(metrics.losing_trades, 0);
        assert_eq!(metrics.win_rate, 100.0);
        assert_eq!(metrics.gross_profit, dec!(200));
        assert_eq!(metrics.average_win, dec!(100));
        assert!(metrics.sharpe_ratio > 0.0);
    }
    
    #[test]
    fn test_metrics_with_mixed_trades() {
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
}
