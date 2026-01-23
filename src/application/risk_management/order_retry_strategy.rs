use crate::application::market_data::spread_cache::SpreadData;
use crate::domain::trading::types::{Order, OrderSide, OrderType};
use rust_decimal::Decimal;

use rust_decimal_macros::dec;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    pub limit_timeout_ms: u64,
    pub enable_retry: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            limit_timeout_ms: 5000,
            enable_retry: true,
        }
    }
}

pub struct OrderRetryStrategy {
    #[allow(dead_code)] // Will be used for future stateful retry logic
    retry_config: RetryConfig,
}

impl OrderRetryStrategy {
    pub fn new(retry_config: RetryConfig) -> Self {
        Self { retry_config }
    }

    /// Create a liquidation order, preferring Limit orders if possible.
    ///
    /// # Logic
    /// 1. If Panic Mode (no price data) -> Market Order
    /// 2. If Spread Data available -> Limit Order at Mid Price
    /// 3. Fallback -> Market Order
    pub fn create_liquidation_order(
        &self,
        symbol: &str,
        side: OrderSide,
        quantity: Decimal,
        spread_data: Option<SpreadData>,
        panic_mode: bool,
    ) -> Order {
        if panic_mode {
            return self.create_market_order(symbol, side, quantity);
        }

        if let Some(spread) = spread_data {
            // Attempt Limit Order at Mid Price
            let mid_price = (spread.bid + spread.ask) / dec!(2.0);

            return Order {
                id: Uuid::new_v4().to_string(),
                symbol: symbol.to_string(),
                side,
                price: mid_price,
                quantity,
                order_type: OrderType::Limit,
                timestamp: chrono::Utc::now().timestamp_millis(),
            };
        }

        // Fallback to Market
        self.create_market_order(symbol, side, quantity)
    }

    fn create_market_order(&self, symbol: &str, side: OrderSide, quantity: Decimal) -> Order {
        Order {
            id: Uuid::new_v4().to_string(),
            symbol: symbol.to_string(),
            side,
            price: Decimal::ZERO,
            quantity,
            order_type: OrderType::Market,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}
