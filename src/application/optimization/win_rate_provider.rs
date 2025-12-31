
use async_trait::async_trait;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{info, warn};

use crate::domain::repositories::TradeRepository;
use crate::domain::trading::types::{Order, OrderSide};

/// Trait to provide win rate for a given symbol
#[async_trait]
pub trait WinRateProvider: Send + Sync {
    /// Get the win rate for a symbol (0.0 to 1.0)
    async fn get_win_rate(&self, symbol: &str) -> f64;
}

/// Static win rate provider for testing or safe defaults
pub struct StaticWinRateProvider {
    win_rate: f64,
}

impl StaticWinRateProvider {
    pub fn new(win_rate: f64) -> Self {
        Self { win_rate }
    }
}

#[async_trait]
impl WinRateProvider for StaticWinRateProvider {
    async fn get_win_rate(&self, _symbol: &str) -> f64 {
        self.win_rate
    }
}

/// Historical win rate provider based on actual trade history
pub struct HistoricalWinRateProvider {
    repository: Arc<dyn TradeRepository>,
    default_win_rate: f64,
    min_trades: usize, // Minimum trades required to use historical data
}

impl HistoricalWinRateProvider {
    pub fn new(repository: Arc<dyn TradeRepository>, default_win_rate: f64, min_trades: usize) -> Self {
        Self {
            repository,
            default_win_rate,
            min_trades,
        }
    }

    /// Calculate profit/loss for a closed trade pair (simplistic FIFO matching)
    /// Note: This is a robust estimation. Exact PnL usually requires a ledger.
    /// Here we assume if we sold at higher price than average buy price, it's a win.
    /// But `TradeRepository` returns individual Orders (Buy or Sell).
    /// We need to reconstruct "Trades" (Round Trips) to determine wins.
    ///
    /// For V1, we will use a simplified heuristic:
    /// - Fetch recent SELL orders.
    /// - For each SELL, find a corresponding BUY (FIFO) or just assume if Price > Avg Entry it was a win?
    /// - Problem: `Order` struct doesn't store PnL.
    ///
    /// Alternative: The `Analyst` or `System` should persist "CompletedTrades" with PnL.
    /// Current `TradeRepository` only stores `Order`s.
    ///
    /// Workaround for Audit Fix V1:
    /// Check if we have `PerformanceSnapshot` or similar?
    ///
    /// Let's use a simpler heuristic available in `Order` if possible:
    /// Sadly `Order` is just execution.
    ///
    /// Better approach:
    /// Iterate all orders for symbol. Sort by time.
    /// Replay history to calculate PnL of closed positions.
    fn calculate_win_rate_from_orders(orders: &[Order]) -> Option<(f64, usize)> {

        let mut wins = 0;
        let mut total_closed = 0;

        // Simple FIFO Replay
        let mut inventory: Vec<(Decimal, Decimal)> = Vec::new(); // (Price, Qty)
        
        for order in orders {
            match order.side {
                OrderSide::Buy => {
                    inventory.push((order.price, order.quantity));
                }
                OrderSide::Sell => {
                    let mut qty_to_sell = order.quantity;
                    let mut realized_pnl = Decimal::ZERO;
                    
                    while qty_to_sell > Decimal::ZERO && !inventory.is_empty() {
                        let (buy_price, buy_qty) = inventory.remove(0);
                        
                        if buy_qty <= qty_to_sell {
                            // Sold entire lot
                            realized_pnl += (order.price - buy_price) * buy_qty;
                            qty_to_sell -= buy_qty;
                        } else {
                            // Partial sell of lot
                            realized_pnl += (order.price - buy_price) * qty_to_sell;
                            // Put remainder back at front
                            inventory.insert(0, (buy_price, buy_qty - qty_to_sell));
                            qty_to_sell = Decimal::ZERO;
                        }
                    }
                    
                    if qty_to_sell == Decimal::ZERO {
                        // We successfully closed some volume
                         total_closed += 1;
                         if realized_pnl > Decimal::ZERO {
                             wins += 1;
                         }
                    }
                }
            }
        }

        if total_closed == 0 {
            return None;
        }

        Some((wins as f64 / total_closed as f64, total_closed))

    }
}

#[async_trait]
impl WinRateProvider for HistoricalWinRateProvider {
    async fn get_win_rate(&self, symbol: &str) -> f64 {
        let orders = match self.repository.find_by_symbol(symbol).await {
            Ok(o) => o,
            Err(e) => {
                warn!("Failed to fetch history for {}: {}. Using default.", symbol, e);
                return self.default_win_rate;
            }
        };

        // Sort by timestamp just in case
        let mut sorted_orders = orders;
        sorted_orders.sort_by_key(|o| o.timestamp);

        let calculated_rate = Self::calculate_win_rate_from_orders(&sorted_orders);
        
        if let Some((rate, total_closed)) = calculated_rate {
             if total_closed < self.min_trades {
                 return self.default_win_rate;
             }

             // Weighted blend with default if low sample size?
             // Or just return it if we met min_trades threshold (sort of).
             // `calculate_win_rate_from_orders` returns None if 0 trades.
             
             // Check if we have enough data points to strictly trust it?
             // The logic inside `calculate_win_rate_from_orders` counts "Sell Events" as trades.
             // Let's trust it for now as "Empirical".
              info!("Empirical Win Rate for {}: {:.2} ({} trades)", symbol, rate, total_closed);
              rate

        } else {
            self.default_win_rate
        }
    }
}
