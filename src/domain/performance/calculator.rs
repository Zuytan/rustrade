use super::stats::Stats;
use crate::domain::trading::types::{Order, OrderSide};
use rust_decimal::Decimal;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionSide {
    Flat,
    Long,
    Short,
}

/// Calculates performance metrics (Sharpe Ratio, Win Rate) from a list of raw orders
/// by reconstructing trades using FIFO matching.
pub fn calculate_metrics_from_orders(orders: &[Order]) -> (Decimal, Decimal) {
    if orders.is_empty() {
        return (Decimal::ZERO, Decimal::ZERO);
    }

    let mut open_chunks: VecDeque<Order> = VecDeque::new();
    let mut current_side = PositionSide::Flat;

    // Map: day_index -> (Realized PnL, Total Entry Value, Wins, Total closed trades)
    let mut daily_stats: HashMap<i64, (Decimal, Decimal, usize, usize)> = HashMap::new();

    for order in orders {
        let mut qty_to_process = order.quantity;
        let price = order.price;

        if current_side == PositionSide::Flat {
            current_side = match order.side {
                OrderSide::Buy => PositionSide::Long,
                OrderSide::Sell => PositionSide::Short,
            };
            open_chunks.push_back(order.clone());
            continue;
        }

        let is_increasing = (current_side == PositionSide::Long && order.side == OrderSide::Buy)
            || (current_side == PositionSide::Short && order.side == OrderSide::Sell);

        if is_increasing {
            open_chunks.push_back(order.clone());
        } else {
            // Decreasing or reversing -> FIFO matching
            // Using milliseconds for timestamp, 86_400_000 ms per day
            let day_index = order.timestamp / 86_400_000;

            while qty_to_process > Decimal::ZERO && !open_chunks.is_empty() {
                let mut chunk = open_chunks.pop_front().unwrap();
                let match_qty = chunk.quantity.min(qty_to_process);
                let entry_price = chunk.price;

                if entry_price > Decimal::ZERO {
                    let chunk_pnl = match current_side {
                        PositionSide::Long => (price - entry_price) * match_qty,
                        PositionSide::Short => (entry_price - price) * match_qty,
                        PositionSide::Flat => unreachable!(),
                    };
                    let chunk_entry_value = entry_price * match_qty;

                    let stat = daily_stats.entry(day_index).or_insert((
                        Decimal::ZERO,
                        Decimal::ZERO,
                        0,
                        0,
                    ));
                    stat.0 += chunk_pnl;
                    stat.1 += chunk_entry_value;
                    stat.3 += 1;
                    if chunk_pnl > Decimal::ZERO {
                        stat.2 += 1;
                    }
                }

                qty_to_process -= match_qty;
                chunk.quantity -= match_qty;

                if chunk.quantity > Decimal::ZERO {
                    open_chunks.push_front(chunk);
                }
            }

            if qty_to_process > Decimal::ZERO {
                // Reversed direction
                current_side = match order.side {
                    OrderSide::Buy => PositionSide::Long,
                    OrderSide::Sell => PositionSide::Short,
                };
                let mut new_chunk = order.clone();
                new_chunk.quantity = qty_to_process;
                open_chunks.push_back(new_chunk);
            } else if open_chunks.is_empty() {
                current_side = PositionSide::Flat;
            }
        }
    }

    let mut total_wins = 0;
    let mut total_trades = 0;
    let mut daily_returns = Vec::new();

    for &(pnl, entry_val, wins, trades) in daily_stats.values() {
        total_wins += wins;
        total_trades += trades;
        if entry_val > Decimal::ZERO {
            daily_returns.push(pnl / entry_val);
        } else {
            daily_returns.push(Decimal::ZERO);
        }
    }

    let win_rate = if total_trades > 0 {
        Decimal::from(total_wins) / Decimal::from(total_trades)
    } else {
        Decimal::ZERO
    };

    // calculate annualized Sharpe ratio
    let sharpe = Stats::sharpe_ratio(&daily_returns, true);

    (sharpe, win_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::OrderType;
    use rust_decimal_macros::dec;

    fn create_order(side: OrderSide, price: Decimal, quantity: Decimal, timestamp: i64) -> Order {
        Order {
            id: "test".to_string(),
            symbol: "TEST".to_string(),
            side,
            price,
            quantity,
            order_type: OrderType::Market,
            status: crate::domain::trading::types::OrderStatus::Filled,
            timestamp,
        }
    }

    const DAY: i64 = 86_400_000;

    #[test]
    fn test_calculate_metrics_simple_win_long() {
        let orders = vec![
            create_order(OrderSide::Buy, dec!(100), dec!(1), DAY),
            create_order(OrderSide::Sell, dec!(110), dec!(1), DAY), // day 1 closed, +10%
            create_order(OrderSide::Buy, dec!(100), dec!(1), DAY * 2),
            create_order(OrderSide::Sell, dec!(110), dec!(1), DAY * 2), // day 2 closed, +10%
        ];

        let (sharpe, win_rate) = calculate_metrics_from_orders(&orders);
        assert_eq!(win_rate, dec!(1.0));
        // Variance of [0.1, 0.1] is 0 -> sharpe is 0.0
        assert_eq!(sharpe, Decimal::ZERO);
    }

    #[test]
    fn test_calculate_metrics_short_selling() {
        let orders = vec![
            create_order(OrderSide::Sell, dec!(100), dec!(1), DAY),
            create_order(OrderSide::Buy, dec!(90), dec!(1), DAY), // day 1 closed Short, +10%
            create_order(OrderSide::Sell, dec!(100), dec!(1), DAY * 2),
            create_order(OrderSide::Buy, dec!(110), dec!(1), DAY * 2), // day 2 closed Short, -10%
        ];

        let (_sharpe, win_rate) = calculate_metrics_from_orders(&orders);
        assert_eq!(win_rate, dec!(0.5));
    }

    #[test]
    fn test_calculate_metrics_positive_sharpe() {
        let orders = vec![
            create_order(OrderSide::Buy, dec!(100), dec!(1), DAY),
            create_order(OrderSide::Sell, dec!(110), dec!(1), DAY), // day 1 returns +10%
            create_order(OrderSide::Buy, dec!(100), dec!(1), DAY * 2),
            create_order(OrderSide::Sell, dec!(105), dec!(1), DAY * 2), // day 2 returns +5%
        ];

        let (sharpe, _win_rate) = calculate_metrics_from_orders(&orders);
        // Returns are 0.1 and 0.05. Mean = 0.075. Sharpe is > 1.0.
        assert!(sharpe > dec!(1.0));
    }

    #[test]
    fn test_calculate_metrics_reversal() {
        // Go long 1 @ 100 on Day 1
        let o1 = create_order(OrderSide::Buy, dec!(100), dec!(1), DAY);
        // Sell 2 @ 110 on Day 1 -> Closes Long 1 (PnL +10), opens Short 1
        let o2 = create_order(OrderSide::Sell, dec!(110), dec!(2), DAY);
        // Buy 1 @ 100 on Day 2 -> Closes Short 1 (PnL +10)
        let o3 = create_order(OrderSide::Buy, dec!(100), dec!(1), DAY * 2);

        let orders = vec![o1, o2, o3];

        let (sharpe, win_rate) = calculate_metrics_from_orders(&orders);
        assert_eq!(win_rate, dec!(1.0));
        // Day 1 return = +10 / 100 = 10%.
        // Day 2 return = +10 / 110 = 9.09%.
        // Returns positive, mean > 0, standard dev small. Sharpe should be > 0.
        assert!(sharpe >= Decimal::ZERO);
    }
}
