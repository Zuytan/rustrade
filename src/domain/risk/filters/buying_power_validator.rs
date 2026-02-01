use async_trait::async_trait;

use tracing::debug;

use crate::domain::risk::filters::validator_trait::{
    RiskValidator, ValidationContext, ValidationResult,
};
use crate::domain::trading::types::OrderSide;

/// Configuration for buying power validation
#[derive(Debug, Clone)]
pub struct BuyingPowerConfig {
    /// Whether to strictly enforce buying power checks
    pub enabled: bool,
}

impl Default for BuyingPowerConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Validates that there is sufficient buying power (available cash) for the trade.
///
/// This validator prevents "Insufficient Funds" errors from the broker by
/// checking available cash (Cash - Reservations) against the estimated order cost.
pub struct BuyingPowerValidator {
    config: BuyingPowerConfig,
}

impl BuyingPowerValidator {
    pub fn new(config: BuyingPowerConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl RiskValidator for BuyingPowerValidator {
    fn name(&self) -> &str {
        "BuyingPowerValidator"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        if !self.config.enabled {
            return ValidationResult::Approve;
        }

        // Only validate Buy orders (sells generate cash)
        if !matches!(ctx.proposal.side, OrderSide::Buy) {
            return ValidationResult::Approve;
        }

        // Calculate estimated cost
        // Note: For market orders, this is an estimate. Using proposal price (which is usually last trade or ticker price).
        let estimated_cost = ctx.get_proposal_price() * ctx.proposal.quantity;

        // Check against available cash
        if estimated_cost > ctx.available_cash {
            debug!(
                "BuyingPowerValidator: Insufficient funds. Cost: {}, Available: {}",
                estimated_cost, ctx.available_cash
            );
            return ValidationResult::Reject(format!(
                "Insufficient buying power. Cost: {}, Available: {}",
                estimated_cost, ctx.available_cash
            ));
        }

        ValidationResult::Approve
    }

    fn priority(&self) -> u8 {
        10 // Basic check, should run early
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::state::RiskState;
    use crate::domain::trading::portfolio::Portfolio;
    use crate::domain::trading::types::{OrderType, TradeProposal};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn create_test_proposal(side: OrderSide, price: Decimal, qty: Decimal) -> TradeProposal {
        TradeProposal {
            symbol: "ABC".to_string(),
            side,
            price,
            quantity: qty,
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_approve_sufficient_funds() {
        let validator = BuyingPowerValidator::new(BuyingPowerConfig::default());
        let proposal = create_test_proposal(OrderSide::Buy, dec!(100), dec!(5)); // Cost 500

        // Mock Context
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(1000), // Equity
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(600), // Available Cash > 500,
            None,      // recent_candles
        );

        assert!(validator.validate(&ctx).await.is_approved());
    }

    #[tokio::test]
    async fn test_reject_insufficient_funds() {
        let validator = BuyingPowerValidator::new(BuyingPowerConfig::default());
        let proposal = create_test_proposal(OrderSide::Buy, dec!(100), dec!(10)); // Cost 1000

        // Mock Context
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        // Equity is high ($2000), but Cash is low ($500)
        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(2000), // Equity
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(500), // Available Cash < 1000,
            None,      // recent_candles
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(
            result
                .rejection_reason()
                .unwrap()
                .contains("Insufficient buying power")
        );
    }

    #[tokio::test]
    async fn test_approve_sell_regardless_of_cash() {
        let validator = BuyingPowerValidator::new(BuyingPowerConfig::default());
        let proposal = create_test_proposal(OrderSide::Sell, dec!(100), dec!(10));

        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(2000),
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(0), // Zero Cash
            None,    // recent_candles
        );

        assert!(validator.validate(&ctx).await.is_approved());
    }
}
