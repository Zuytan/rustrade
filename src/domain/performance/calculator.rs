use super::stats::Stats;
use crate::domain::trading::types::{Order, OrderSide};
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use std::collections::VecDeque;

/// Calculates performance metrics (Sharpe Ratio, Win Rate) from a list of raw orders
/// by reconstructing trades using FIFO matching.
pub fn calculate_metrics_from_orders(orders: &[Order]) -> (f64, f64) {
    if orders.is_empty() {
        return (0.0, 0.0);
    }

    // FIFO PnL reconstruction logic
    let mut buys: VecDeque<Order> = VecDeque::new();
    let mut trade_returns = Vec::new();
    let mut wins = 0;
    let mut total_closed_trades = 0;

    for order in orders {
        match order.side {
            OrderSide::Buy => buys.push_back(order.clone()),
            OrderSide::Sell => {
                let mut qty_to_close = order.quantity;
                let exit_price = order.price.to_f64().unwrap_or(0.0);

                while qty_to_close > Decimal::ZERO && !buys.is_empty() {
                    let mut buy = buys
                        .pop_front()
                        .expect("buys.is_empty() checked in while condition");

                    let match_qty = buy.quantity.min(qty_to_close);
                    let entry_price = buy.price.to_f64().unwrap_or(0.0);

                    if entry_price > 0.0 {
                        // Return for this chunk
                        let pnl_pct = (exit_price - entry_price) / entry_price;
                        trade_returns.push(pnl_pct);

                        if pnl_pct > 0.0 {
                            wins += 1;
                        }
                        total_closed_trades += 1;
                    }

                    qty_to_close -= match_qty;
                    buy.quantity -= match_qty;

                    if buy.quantity > Decimal::ZERO {
                        buys.push_front(buy);
                    }
                }
            }
        }
    }

    let win_rate = if total_closed_trades > 0 {
        wins as f64 / total_closed_trades as f64
    } else {
        0.0
    };

    let (sharpe, win_rate) = (Stats::sharpe_ratio(&trade_returns, false), win_rate);

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

    #[test]
    fn test_calculate_metrics_simple_win() {
        let orders = vec![
            create_order(OrderSide::Buy, dec!(100), dec!(1), 0),
            create_order(OrderSide::Sell, dec!(110), dec!(1), 0), // +10%
            create_order(OrderSide::Buy, dec!(100), dec!(1), 0),
            create_order(OrderSide::Sell, dec!(110), dec!(1), 0), // +10%
        ];

        let (sharpe, win_rate) = calculate_metrics_from_orders(&orders);

        assert_eq!(win_rate, 1.0); // 100% win rate
        // Sharpe undefined for constant return (std_dev = 0), code returns 0.0
        assert_eq!(sharpe, 0.0);
    }

    #[test]
    fn test_calculate_metrics_mixed() {
        let orders = vec![
            create_order(OrderSide::Buy, dec!(100), dec!(1), 0),
            create_order(OrderSide::Sell, dec!(110), dec!(1), 0), // +10%
            create_order(OrderSide::Buy, dec!(100), dec!(1), 0),
            create_order(OrderSide::Sell, dec!(90), dec!(1), 0), // -10%
        ];

        // Mean = 0. StdDev = 0.1414... (approx) (variance = ((0.1-0)^2 + (-0.1-0)^2)/1 = 0.02. sqrt(0.02) ~ 0.1414)
        // Sharpe = 0 / 0.1414 = 0

        let (sharpe, win_rate) = calculate_metrics_from_orders(&orders);

        assert_eq!(win_rate, 0.5);
        assert!((sharpe).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_metrics_positive_sharpe() {
        let orders = vec![
            create_order(OrderSide::Buy, dec!(100), dec!(1), 0),
            create_order(OrderSide::Sell, dec!(110), dec!(1), 0), // +10%
            create_order(OrderSide::Buy, dec!(100), dec!(1), 0),
            create_order(OrderSide::Sell, dec!(105), dec!(1), 0), // +5%
        ];
        // Mean = 7.5%. Var = ((10-7.5)^2 + (5-7.5)^2)/1 = (6.25 + 6.25) = 12.5. StdDev = 3.53% (0.0353)
        // Sharpe = 0.075 / 0.0353 ~ 2.12

        let (sharpe, _win_rate) = calculate_metrics_from_orders(&orders);
        assert!(sharpe > 1.0);
    }
}
