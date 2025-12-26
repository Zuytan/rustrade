use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl fmt::Display for OrderSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "BUY"),
            OrderSide::Sell => write!(f, "SELL"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketEvent {
    Quote {
        symbol: String,
        price: Decimal,
        timestamp: i64,
    },
    // Can add Trade, OrderBookUpdate, etc.
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TradeProposal {
    pub symbol: String,
    pub side: OrderSide,
    pub price: Decimal,
    pub quantity: Decimal,
    pub reason: String,
    pub timestamp: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Order {
    pub id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub price: Decimal,
    pub quantity: Decimal,
    pub timestamp: i64,
}

/// Represents a completed trade with profit/loss information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub entry_price: Decimal,
    pub exit_price: Option<Decimal>,
    pub quantity: Decimal,
    pub pnl: Decimal, // Realized profit/loss
    pub entry_timestamp: i64,
    pub exit_timestamp: Option<i64>,
}

impl Trade {
    /// Create a new trade from an opening order
    pub fn from_order(order: &Order) -> Self {
        Self {
            id: order.id.clone(),
            symbol: order.symbol.clone(),
            side: order.side,
            entry_price: order.price,
            exit_price: None,
            quantity: order.quantity,
            pnl: Decimal::ZERO,
            entry_timestamp: order.timestamp,
            exit_timestamp: None,
        }
    }

    /// Close the trade and calculate P&L
    pub fn close(&mut self, exit_price: Decimal, exit_timestamp: i64) {
        self.exit_price = Some(exit_price);
        self.exit_timestamp = Some(exit_timestamp);

        // Calculate P&L: (exit - entry) * quantity for buy, (entry - exit) * quantity for sell
        self.pnl = match self.side {
            OrderSide::Buy => (exit_price - self.entry_price) * self.quantity,
            OrderSide::Sell => (self.entry_price - exit_price) * self.quantity,
        };
    }
}

/// Lifecycle state of a position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionLifecycle {
    /// Position has been opened
    Opened,
    /// Position is active and being monitored
    Active,
    /// Position has been closed
    Closed,
}
