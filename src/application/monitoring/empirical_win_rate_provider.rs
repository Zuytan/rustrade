use crate::domain::repositories::TradeRepository;
use crate::domain::trading::types::{Order, OrderSide};
use rust_decimal::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// Calculates empirical win rates from historical trade data
///
/// This service queries the trade repository to calculate actual win rates
/// based on past performance by matching buy/sell order pairs.
///
/// Win rates are calculated per symbol by pairing buy and sell orders
/// and determining if the trade was profitable.
pub struct EmpiricalWinRateProvider {
    trade_repository: Arc<dyn TradeRepository>,
    default_win_rate: f64,
    min_trades_threshold: usize,
}

impl EmpiricalWinRateProvider {
    /// Create a new EmpiricalWinRateProvider
    ///
    /// # Arguments
    /// * `trade_repository` - Repository for accessing historical orders
    /// * `default_win_rate` - Fallback win rate when insufficient data (e.g., 0.50)
    /// * `min_trades_threshold` - Minimum trades required for empirical calculation
    pub fn new(
        trade_repository: Arc<dyn TradeRepository>,
        default_win_rate: f64,
        min_trades_threshold: usize,
    ) -> Self {
        Self {
            trade_repository,
            default_win_rate,
            min_trades_threshold,
        }
    }

    /// Reconstruct completed trades from buy/sell order pairs
    ///
    /// Matches buy orders with subsequent sell orders for the same symbol.
    /// Returns vector of (buy_price, sell_price, quantity) tuples.
    async fn reconstruct_trades(&self, symbol: &str) -> Vec<(rust_decimal::Decimal, rust_decimal::Decimal, rust_decimal::Decimal)> {
        match self.trade_repository.find_by_symbol(symbol).await {
            Ok(orders) => {
                let mut trades = Vec::new();
                let mut open_position: Option<&Order> = None;

                for order in &orders {
                    match order.side {
                        OrderSide::Buy => {
                            open_position = Some(order);
                        }
                        OrderSide::Sell => {
                            if let Some(buy_order) = open_position {
                                trades.push((buy_order.price, order.price, order.quantity));
                                open_position = None;
                            }
                        }
                    }
                }

                trades
            }
            Err(e) => {
                info!("EmpiricalWinRate: Failed to fetch orders for {}: {}", symbol, e);
                Vec::new()
            }
        }
    }

    /// Calculate win rate for a specific symbol
    ///
    /// Returns empirical win rate if sufficient trades exist,
    /// otherwise returns default conservative estimate.
    ///
    /// # Arguments
    /// * `symbol` - Symbol to calculate win rate for
    ///
    /// # Returns
    /// Win rate as a decimal (e.g., 0.55 = 55% win rate)
    pub async fn get_win_rate(&self, symbol: &str) -> f64 {
        let trades = self.reconstruct_trades(symbol).await;

        if trades.len() < self.min_trades_threshold {
            info!(
                "EmpiricalWinRate: Insufficient data for {} ({} trades < {} threshold), using default {:.2}%",
                symbol,
                trades.len(),
                self.min_trades_threshold,
                self.default_win_rate * 100.0
            );
            return self.default_win_rate;
        }

        let winning_trades = trades
            .iter()
            .filter(|(buy_price, sell_price, _qty)| sell_price > buy_price)
            .count();

        let win_rate = winning_trades as f64 / trades.len() as f64;

        info!(
            "EmpiricalWinRate: {} - {}/{} trades won = {:.2}%",
            symbol,
            winning_trades,
            trades.len(),
            win_rate * 100.0
        );

        win_rate
    }

    /// Calculate overall win rate across all symbols
    pub async fn get_overall_win_rate(&self) -> f64 {
        match self.trade_repository.get_all().await {
            Ok(all_orders) => {
                // Group by symbol and reconstruct trades
                let mut symbol_groups: HashMap<String, Vec<&Order>> = HashMap::new();
                for order in &all_orders {
                    symbol_groups
                        .entry(order.symbol.clone())
                        .or_insert_with(Vec::new)
                        .push(order);
                }

                let mut total_trades = 0;
                let mut winning_trades = 0;

                for (_symbol, orders) in symbol_groups {
                    let mut open_position: Option<&Order> = None;

                    for order in orders {
                        match order.side {
                            OrderSide::Buy => {
                                open_position = Some(order);
                            }
                            OrderSide::Sell => {
                                if let Some(buy_order) = open_position {
                                    total_trades += 1;
                                    if order.price > buy_order.price {
                                        winning_trades += 1;
                                    }
                                    open_position = None;
                                }
                            }
                        }
                    }
                }

                if total_trades < self.min_trades_threshold {
                    info!(
                        "EmpiricalWinRate: Insufficient overall data ({} trades), using default {:.2}%",
                        total_trades,
                        self.default_win_rate * 100.0
                    );
                    return self.default_win_rate;
                }

                let win_rate = winning_trades as f64 / total_trades as f64;

                info!(
                    "EmpiricalWinRate: Overall - {}/{} trades won = {:.2}%",
                    winning_trades, total_trades, win_rate * 100.0
                );

                win_rate
            }
            Err(e) => {
                info!(
                    "EmpiricalWinRate: Failed to fetch overall orders: {}. Using default {:.2}%",
                    e, self.default_win_rate * 100.0
                );
                self.default_win_rate
            }
        }
    }

    /// Get statistics summary for a symbol
    pub async fn get_statistics(&self, symbol: &str) -> TradeStatistics {
        let trades = self.reconstruct_trades(symbol).await;

        if trades.is_empty() {
            return TradeStatistics::default();
        }

        let total_trades = trades.len();
        let mut total_profit = rust_decimal::Decimal::ZERO;
        let mut total_loss = rust_decimal::Decimal::ZERO;
        let mut winning_trades = 0;

        for (buy_price, sell_price, quantity) in &trades {
            let pnl = (sell_price - buy_price) * quantity;
            if pnl > rust_decimal::Decimal::ZERO {
                winning_trades += 1;
                total_profit += pnl;
            } else {
                total_loss += pnl.abs();
            }
        }

        let losing_trades = total_trades - winning_trades;
        let win_rate = winning_trades as f64 / total_trades as f64;

        let avg_profit = if winning_trades > 0 {
            (total_profit / rust_decimal::Decimal::from(winning_trades))
                .to_f64()
                .unwrap_or(0.0)
        } else {
            0.0
        };

        let avg_loss = if losing_trades > 0 {
            (total_loss / rust_decimal::Decimal::from(losing_trades))
                .to_f64()
                .unwrap_or(0.0)
        } else {
            0.0
        };

        let profit_factor = if total_loss > rust_decimal::Decimal::ZERO {
            (total_profit / total_loss).to_f64().unwrap_or(0.0)
        } else {
            0.0
        };

        TradeStatistics {
            symbol: symbol.to_string(),
            total_trades,
            winning_trades,
            losing_trades,
            win_rate,
            avg_profit,
            avg_loss,
            profit_factor,
            total_profit: total_profit.to_f64().unwrap_or(0.0),
            total_loss: total_loss.to_f64().unwrap_or(0.0),
        }
    }
}

/// Detailed trade statistics for a symbol
#[derive(Debug, Clone)]
pub struct TradeStatistics {
    pub symbol: String,
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub avg_profit: f64,
    pub avg_loss: f64,
    pub profit_factor: f64,
    pub total_profit: f64,
    pub total_loss: f64,
}

impl Default for TradeStatistics {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            total_trades: 0,
            winning_trades: 0,
            losing_trades: 0,
            win_rate: 0.0,
            avg_profit: 0.0,
            avg_loss: 0.0,
            profit_factor: 0.0,
            total_profit: 0.0,
            total_loss: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rust_decimal_macros::dec;

    struct MockTradeRepository {
        orders: Vec<Order>,
    }

    #[async_trait]
    impl TradeRepository for MockTradeRepository {
        async fn save(&self, _trade: &Order) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_symbol(&self, symbol: &str) -> anyhow::Result<Vec<Order>> {
            Ok(self
                .orders
                .iter()
                .filter(|o| o.symbol == symbol)
                .cloned()
                .collect())
        }

        async fn find_recent(&self, _limit: usize) -> anyhow::Result<Vec<Order>> {
            Ok(self.orders.clone())
        }

        async fn get_all(&self) -> anyhow::Result<Vec<Order>> {
            Ok(self.orders.clone())
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(self.orders.len())
        }
    }

    fn create_buy_order(symbol: &str, price: rust_decimal::Decimal) -> Order {
        Order {
            id: uuid::Uuid::new_v4().to_string(),
            symbol: symbol.to_string(),
            side: OrderSide::Buy,
            price,
            quantity: dec!(10.0),
            order_type: crate::domain::trading::types::OrderType::Market,
            timestamp: 0,
        }
    }

    fn create_sell_order(symbol: &str, price: rust_decimal::Decimal) -> Order {
        Order {
            id: uuid::Uuid::new_v4().to_string(),
            symbol: symbol.to_string(),
            side: OrderSide::Sell,
            price,
            quantity: dec!(10.0),
            order_type: crate::domain::trading::types::OrderType::Market,
            timestamp: 1000,
        }
    }

    #[tokio::test]
    async fn test_sufficient_data_returns_empirical_win_rate() {
        let orders = vec![
            create_buy_order("AAPL", dec!(100.0)),
            create_sell_order("AAPL", dec!(105.0)), // Win
            create_buy_order("AAPL", dec!(100.0)),
            create_sell_order("AAPL", dec!(95.0)), // Loss
            create_buy_order("AAPL", dec!(100.0)),
            create_sell_order("AAPL", dec!(103.0)), // Win
        ];

        let repo = Arc::new(MockTradeRepository { orders });
        let provider = EmpiricalWinRateProvider::new(repo, 0.50, 2);

        let win_rate = provider.get_win_rate("AAPL").await;
        assert!((win_rate - 0.666).abs() < 0.01); // 2/3 ≈ 66.7%
    }

    #[tokio::test]
    async fn test_insufficient_data_returns_default() {
        let orders = vec![
            create_buy_order("AAPL", dec!(100.0)),
            create_sell_order("AAPL", dec!(105.0)), // Only 1 trade
        ];

        let repo = Arc::new(MockTradeRepository { orders });
        let provider = EmpiricalWinRateProvider::new(repo, 0.50, 5); // Threshold: 5

        let win_rate = provider.get_win_rate("AAPL").await;
        assert_eq!(win_rate, 0.50); // Default
    }

    #[tokio::test]
    async fn test_statistics_calculation() {
        let orders = vec![
            create_buy_order("AAPL", dec!(100.0)),
            create_sell_order("AAPL", dec!(110.0)), // +100 profit
            create_buy_order("AAPL", dec!(100.0)),
            create_sell_order("AAPL", dec!(90.0)), // -100 loss
            create_buy_order("AAPL", dec!(100.0)),
            create_sell_order("AAPL", dec!(105.0)), // +50 profit
        ];

        let repo = Arc::new(MockTradeRepository { orders });
        let provider = EmpiricalWinRateProvider::new(repo, 0.50, 1);

        let stats = provider.get_statistics("AAPL").await;

        assert_eq!(stats.total_trades, 3);
        assert_eq!(stats.winning_trades, 2);
        assert_eq!(stats.losing_trades, 1);
        assert!((stats.win_rate - 0.666).abs() < 0.01);
        assert!((stats.avg_profit - 75.0).abs() < 0.1); // (100 + 50) / 2
        assert!((stats.avg_loss - 100.0).abs() < 0.1);
        assert!((stats.profit_factor - 1.5).abs() < 0.01); // 150 / 100
    }
}
