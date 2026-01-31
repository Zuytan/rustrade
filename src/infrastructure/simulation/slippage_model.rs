use crate::domain::trading::types::OrderSide;
use rand::Rng;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::prelude::ToPrimitive;

/// Trait defining a slippage simulation model.
pub trait SlippageModel: Send + Sync {
    /// Calculates the effective execution price based on theoretical price and quantity.
    fn calculate_execution_price(
        &self,
        price: Decimal,
        quantity: Decimal,
        side: OrderSide,
    ) -> Decimal;
}

/// Volatility-based slippage model.
/// The higher the volatility factor, the wider the potential spread/slippage range.
/// Currently simplistic: random variation within +/- volatility factor.
/// Could be improved to be quantity-dependent (Impact Model).
#[derive(Debug, Clone)]
pub struct VolatilitySlippage {
    /// Slippage volatility factor (e.g. 0.0005 for 5bps)
    volatility_factor: f64,
}

impl VolatilitySlippage {
    pub fn new(volatility_factor: f64) -> Self {
        Self { volatility_factor }
    }
}

impl SlippageModel for VolatilitySlippage {
    fn calculate_execution_price(
        &self,
        price: Decimal,
        _quantity: Decimal,
        side: OrderSide,
    ) -> Decimal {
        let mut rng = rand::rng();

        // Random slippage percentage: usually negative for the trader (price gets worse)
        // But in volatile markets, it can occasionally be positive (positive slippage), though rare.
        // Here we bias towards negative slippage (worse price).
        // Bias: -0.5 to +0.5 of range -> mostly negative impact?
        // Let's model it as: Price * (1 +/- random(volatility))
        // But 'Slippage' strictly speaking implies worse price. Market impact is always against you.
        // Let's assume 80% chance of worse price, 20% chance of better or equal.

        let noise = rng.random_range(-self.volatility_factor..=self.volatility_factor);

        // Apply bias: shift noise to be mostly unfavorable
        // E.g. add an "impact cost"
        let impact = self.volatility_factor * 0.2; // Fixed cost

        // Effective change pct
        // Buy: Price increases (Bad) -> +impact
        // Sell: Price decreases (Bad) -> -impact

        let pct_change = match side {
            OrderSide::Buy => impact + noise,
            OrderSide::Sell => -(impact + noise),
        };

        // Ensure price doesn't go below zero
        let new_price_f64 = price.to_f64().unwrap_or(0.0) * (1.0 + pct_change);

        Decimal::from_f64(new_price_f64).unwrap_or(price)
    }
}

/// No Slippage model (perfect execution).
pub struct ZeroSlippage;

impl SlippageModel for ZeroSlippage {
    fn calculate_execution_price(
        &self,
        price: Decimal,
        _quantity: Decimal,
        _side: OrderSide,
    ) -> Decimal {
        price
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volatility_slippage() {
        let model = VolatilitySlippage::new(0.01); // 1% volatility
        let price = Decimal::from(100);
        let qty = Decimal::from(1);

        // Run multiple times to check range
        for _ in 0..100 {
            let exec_price = model.calculate_execution_price(price, qty, OrderSide::Buy);
            let diff = (exec_price - price).abs();
            // Should be roughly within 1% + impact bias
            // Impact is 0.2% -> 1.2% effective max deviation roughly
            assert!(diff < Decimal::from_f64_retain(2.0).unwrap());
        }
    }
}
