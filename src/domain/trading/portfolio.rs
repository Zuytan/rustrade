use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct Portfolio {
    pub cash: Decimal,
    pub positions: HashMap<String, Position>,
    pub realized_pnl: Decimal, // Track total realized profit/loss
    pub trade_history: Vec<crate::domain::trading::types::Trade>, // Complete audit trail
    pub starting_cash: Decimal,

    pub max_equity: Decimal,
    pub day_trades_count: u64, // Added for PDT tracking
    pub synchronized: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub symbol: String,
    pub quantity: Decimal,
    pub average_price: Decimal,
}

impl Portfolio {
    pub fn new() -> Self {
        Self {
            cash: Decimal::ZERO,
            positions: HashMap::new(),
            realized_pnl: Decimal::ZERO,
            trade_history: Vec::new(),
            starting_cash: Decimal::ZERO,

            max_equity: Decimal::ZERO,
            day_trades_count: 0,
            synchronized: false,
        }
    }
}

impl Default for Portfolio {
    fn default() -> Self {
        Self::new()
    }
}

impl Portfolio {
    /// Calculate total equity (cash + unrealized position value)
    pub fn total_equity(&self, current_prices: &HashMap<String, Decimal>) -> Decimal {
        let mut equity = self.cash;

        for (symbol, position) in &self.positions {
            if let Some(&current_price) = current_prices.get(symbol) {
                equity += position.quantity * current_price;
            } else {
                // If no current price available, use average price (conservative)
                equity += position.quantity * position.average_price;
            }
        }

        equity
    }

    /// Calculate unrealized P&L for all positions
    pub fn unrealized_pnl(&self, current_prices: &HashMap<String, Decimal>) -> Decimal {
        let mut unrealized = Decimal::ZERO;

        for (symbol, position) in &self.positions {
            if let Some(&current_price) = current_prices.get(symbol) {
                let position_value = position.quantity * current_price;
                let cost_basis = position.quantity * position.average_price;
                unrealized += position_value - cost_basis;
            }
        }

        unrealized
    }

    /// Record a completed trade and update realized P&L
    pub fn record_trade(&mut self, trade: crate::domain::trading::types::Trade) {
        self.realized_pnl += trade.pnl;
        self.trade_history.push(trade);
    }

    /// Get total P&L (realized + unrealized)
    pub fn total_pnl(&self, current_prices: &HashMap<String, Decimal>) -> Decimal {
        self.realized_pnl + self.unrealized_pnl(current_prices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_total_equity_calculation() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(10000);

        // Add position: 10 shares at $100 average
        portfolio.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: dec!(10),
                average_price: dec!(100),
            },
        );

        let mut current_prices = HashMap::new();
        current_prices.insert("AAPL".to_string(), dec!(110)); // Price went up to $110

        // Total equity = cash + (quantity * current_price)
        // = 10000 + (10 * 110) = 11100
        assert_eq!(portfolio.total_equity(&current_prices), dec!(11100));
    }

    #[test]
    fn test_unrealized_pnl_profit() {
        let mut portfolio = Portfolio::new();

        // Buy 10 shares at $100
        portfolio.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: dec!(10),
                average_price: dec!(100),
            },
        );

        let mut current_prices = HashMap::new();
        current_prices.insert("AAPL".to_string(), dec!(110)); // Price increased

        // Unrealized P&L = (current - entry) * quantity
        // = (110 - 100) * 10 = 100
        assert_eq!(portfolio.unrealized_pnl(&current_prices), dec!(100));
    }

    #[test]
    fn test_unrealized_pnl_loss() {
        let mut portfolio = Portfolio::new();

        portfolio.positions.insert(
            "TSLA".to_string(),
            Position {
                symbol: "TSLA".to_string(),
                quantity: dec!(5),
                average_price: dec!(200),
            },
        );

        let mut current_prices = HashMap::new();
        current_prices.insert("TSLA".to_string(), dec!(180)); // Price decreased

        // Unrealized P&L = (180 - 200) * 5 = -100
        assert_eq!(portfolio.unrealized_pnl(&current_prices), dec!(-100));
    }

    #[test]
    fn test_record_trade_updates_realized_pnl() {
        let mut portfolio = Portfolio::new();

        let trade = crate::domain::trading::types::Trade {
            id: "1".to_string(),
            symbol: "NVDA".to_string(),
            side: crate::domain::trading::types::OrderSide::Buy,
            entry_price: dec!(100),
            exit_price: Some(dec!(120)),
            quantity: dec!(10),
            pnl: dec!(200), // Profit of $200 (net of fees)
            entry_timestamp: 1000,
            exit_timestamp: Some(2000),
            strategy_used: None,
            regime_detected: None,
            entry_reason: None,
            exit_reason: None,
            slippage: None,
            fees: dec!(0),
        };

        portfolio.record_trade(trade.clone());

        assert_eq!(portfolio.realized_pnl, dec!(200));
        assert_eq!(portfolio.trade_history.len(), 1);
    }

    #[test]
    fn test_total_pnl_combines_realized_and_unrealized() {
        let mut portfolio = Portfolio::new();
        portfolio.realized_pnl = dec!(500); // Already made $500

        // Open position with unrealized profit
        portfolio.positions.insert(
            "BTC".to_string(),
            Position {
                symbol: "BTC".to_string(),
                quantity: dec!(1),
                average_price: dec!(50000),
            },
        );

        let mut current_prices = HashMap::new();
        current_prices.insert("BTC".to_string(), dec!(52000)); // +2000 unrealized

        // Total P&L = 500 (realized) + 2000 (unrealized) = 2500
        assert_eq!(portfolio.total_pnl(&current_prices), dec!(2500));
    }
}
