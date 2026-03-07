use crate::domain::trading::types::OrderSide;
use rust_decimal::Decimal;
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq)]
pub struct TradeCost {
    pub fee: Decimal,
    pub slippage_cost: Decimal,
    pub total_impact: Decimal,
}

pub trait FeeModel: Debug + Send + Sync {
    /// Calculate estimated cost for a trade
    fn calculate_cost(&self, quantity: Decimal, price: Decimal, side: OrderSide) -> TradeCost;

    /// Calculate funding cost for holding a position over time (e.g. for crypto perpetuals)
    /// Returns the funding fee (positive means you pay, negative means you receive)
    fn calculate_funding_cost(
        &self,
        quantity: Decimal,
        price: Decimal,
        hold_time_hours: Decimal,
    ) -> Decimal {
        let _ = (quantity, price, hold_time_hours); // default no-op
        Decimal::ZERO
    }

    /// Get description of the fee model
    fn description(&self) -> String;
}

#[derive(Debug, Clone)]
pub struct ConstantFeeModel {
    pub commission_per_share: Decimal,
    pub slippage_pct: Decimal,
}

impl ConstantFeeModel {
    pub fn new(commission_per_share: Decimal, slippage_pct: Decimal) -> Self {
        Self {
            commission_per_share,
            slippage_pct,
        }
    }
}

impl FeeModel for ConstantFeeModel {
    fn calculate_cost(&self, quantity: Decimal, price: Decimal, _side: OrderSide) -> TradeCost {
        let trade_value = quantity * price;
        let fee = quantity * self.commission_per_share;
        let slippage_cost = trade_value * self.slippage_pct;

        TradeCost {
            fee,
            slippage_cost,
            total_impact: fee + slippage_cost,
        }
    }

    fn description(&self) -> String {
        format!(
            "Constant Fee Model (Com: {}, Slip: {:.2}%)",
            self.commission_per_share,
            self.slippage_pct * Decimal::from(100)
        )
    }
}

#[derive(Debug, Clone)]
pub struct TieredFeeModel {
    pub maker_fee_pct: Decimal,
    pub taker_fee_pct: Decimal,
    pub slippage_pct: Decimal,
}

impl TieredFeeModel {
    pub fn new(maker_fee_pct: Decimal, taker_fee_pct: Decimal, slippage_pct: Decimal) -> Self {
        Self {
            maker_fee_pct,
            taker_fee_pct,
            slippage_pct,
        }
    }
}

impl FeeModel for TieredFeeModel {
    fn calculate_cost(&self, quantity: Decimal, price: Decimal, _side: OrderSide) -> TradeCost {
        let trade_value = quantity * price;
        // Assume taker fee for now as most bot orders are market/aggressive
        let fee = trade_value * self.taker_fee_pct;
        let slippage_cost = trade_value * self.slippage_pct;

        TradeCost {
            fee,
            slippage_cost,
            total_impact: fee + slippage_cost,
        }
    }

    fn description(&self) -> String {
        format!(
            "Tiered Fee Model (Taker: {:.2}%, Slip: {:.2}%)",
            self.taker_fee_pct * Decimal::from(100),
            self.slippage_pct * Decimal::from(100)
        )
    }
}

#[derive(Debug, Clone)]
pub struct FundingRateFeeModel {
    pub base_fee_pct: Decimal,        // Base taker fee (e.g. 0.04% -> 0.0004)
    pub slippage_pct: Decimal,        // Estimated slippage
    pub funding_rate_per_8h: Decimal, // e.g. 0.01% -> 0.0001
}

impl FundingRateFeeModel {
    pub fn new(base_fee_pct: Decimal, slippage_pct: Decimal, funding_rate_per_8h: Decimal) -> Self {
        Self {
            base_fee_pct,
            slippage_pct,
            funding_rate_per_8h,
        }
    }
}

impl FeeModel for FundingRateFeeModel {
    fn calculate_cost(&self, quantity: Decimal, price: Decimal, _side: OrderSide) -> TradeCost {
        let trade_value = quantity * price;
        let fee = trade_value * self.base_fee_pct;
        let slippage_cost = trade_value * self.slippage_pct;

        TradeCost {
            fee,
            slippage_cost,
            total_impact: fee + slippage_cost,
        }
    }

    fn calculate_funding_cost(
        &self,
        quantity: Decimal,
        price: Decimal,
        hold_time_hours: Decimal,
    ) -> Decimal {
        let trade_value = quantity * price;
        // Number of 8-hour intervals the position was held
        use rust_decimal_macros::dec;
        let intervals = hold_time_hours / dec!(8.0);

        trade_value * self.funding_rate_per_8h * intervals
    }

    fn description(&self) -> String {
        use rust_decimal_macros::dec;
        format!(
            "Funding Rate Model (Base: {:.2}%, Slip: {:.2}%, Funding: {:.3}%/8h)",
            self.base_fee_pct * dec!(100),
            self.slippage_pct * dec!(100),
            self.funding_rate_per_8h * dec!(100)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_funding_rate_cost() {
        let model = FundingRateFeeModel::new(dec!(0.0004), dec!(0.0001), dec!(0.0001)); // 0.01% per 8h
        let qty = dec!(1.0);
        let price = dec!(50000.0);

        // Hold for 24 hours = 3 intervals of 8h
        let funding = model.calculate_funding_cost(qty, price, dec!(24.0));
        // 50000 * 0.0001 * 3 = 15.0
        assert_eq!(funding, dec!(15.0));

        let basic_cost = model.calculate_cost(qty, price, OrderSide::Buy);
        assert_eq!(basic_cost.fee, dec!(20.0)); // 50000 * 0.0004
        assert_eq!(basic_cost.total_impact, dec!(25.0)); // 20 + 5
    }
}
