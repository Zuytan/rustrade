use rust_decimal::Decimal;
use crate::domain::trading::types::OrderSide;
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
        format!("Constant Fee Model (Com: {}, Slip: {:.2}%)", self.commission_per_share, self.slippage_pct * Decimal::from(100))
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
        format!("Tiered Fee Model (Taker: {:.2}%, Slip: {:.2}%)", self.taker_fee_pct * Decimal::from(100), self.slippage_pct * Decimal::from(100))
    }
}
