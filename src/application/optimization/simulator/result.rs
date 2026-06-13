use crate::domain::trading::types::{Order, OrderSide, Trade};
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub trades: Vec<Order>,
    pub initial_equity: Decimal,
    pub final_equity: Decimal,
    pub total_return_pct: Decimal,
    pub buy_and_hold_return_pct: Decimal,
    pub daily_closes: Vec<(i64, Decimal)>, // (Timestamp seconds, Close Price)
    pub alpha: f64,
    pub beta: f64,
    pub benchmark_correlation: f64,
}

pub fn local_orders_to_trades(orders: &[Order]) -> Vec<Trade> {
    let mut trades = Vec::new();
    let mut open_positions: std::collections::HashMap<String, &Order> =
        std::collections::HashMap::new();
    for order in orders {
        match order.side {
            OrderSide::Buy => {
                open_positions.insert(order.symbol.clone(), order);
            }
            OrderSide::Sell => {
                if let Some(buy) = open_positions.remove(&order.symbol) {
                    let pnl = (order.price - buy.price) * order.quantity;
                    trades.push(Trade {
                        id: order.id.clone(),
                        symbol: order.symbol.clone(),
                        side: OrderSide::Buy,
                        entry_price: buy.price,
                        exit_price: Some(order.price),
                        quantity: order.quantity,
                        pnl,
                        entry_timestamp: buy.timestamp,
                        exit_timestamp: Some(order.timestamp),
                        strategy_used: None,
                        regime_detected: None,
                        entry_reason: None,
                        exit_reason: None,
                        slippage: None,
                        fees: rust_decimal::Decimal::ZERO,
                    });
                }
            }
        }
    }
    trades
}
