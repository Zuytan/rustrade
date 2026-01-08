use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use tracing::debug;

use crate::domain::risk::filters::validator_trait::{RiskValidator, ValidationContext, ValidationResult};
use crate::domain::sentiment::SentimentClassification;
use crate::domain::trading::types::OrderSide;

/// Configuration for position size validation
#[derive(Debug, Clone)]
pub struct PositionSizeConfig {
    /// Maximum position size as percentage of equity (e.g., 0.25 = 25%)
    pub max_position_size_pct: f64,
}

impl Default for PositionSizeConfig {
    fn default() -> Self {
        Self {
            max_position_size_pct: 0.10, // Conservative 10% default
        }
    }
}

/// Validates that position sizes don't exceed configured limits
/// 
/// This validator ensures that no single position can grow too large relative to
/// total equity. It also applies sentiment-based adjustments, reducing position
/// sizes during periods of extreme market fear.
pub struct PositionSizeValidator {
    config: PositionSizeConfig,
}

impl PositionSizeValidator {
    pub fn new(config: PositionSizeConfig) -> Self {
        Self { config }
    }

    /// Calculate adjusted max position size based on sentiment
    fn calculate_adjusted_limit(&self, ctx: &ValidationContext<'_>, side: OrderSide) -> f64 {
        let mut adjusted_max_pct = self.config.max_position_size_pct;

        // Apply sentiment-based risk adjustment
        if let Some(sentiment) = ctx.current_sentiment {
            // In Extreme Fear, reduce position size by 50% for Long positions
            if side == OrderSide::Buy && sentiment.classification == SentimentClassification::ExtremeFear {
                adjusted_max_pct *= 0.5;
                debug!(
                    "PositionSizeValidator: Extreme Fear ({}) detected. Reducing max position size to {:.2}%",
                    sentiment.value,
                    adjusted_max_pct * 100.0
                );
            }
        }

        // Apply volatility-based adjustment
        if let Some(multiplier) = ctx.volatility_multiplier {
            adjusted_max_pct *= multiplier;
            debug!(
                "PositionSizeValidator: Volatility multiplier {:.2}x applied. Adjusted max position size: {:.2}%",
                multiplier,
                adjusted_max_pct * 100.0
            );
        }

        adjusted_max_pct
    }
}

#[async_trait]
impl RiskValidator for PositionSizeValidator {
    fn name(&self) -> &str {
        "PositionSizeValidator"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        // Only validate Buy orders (sells reduce exposure)
        if !matches!(ctx.proposal.side, OrderSide::Buy) {
            return ValidationResult::Approve;
        }

        // Skip validation if equity is zero or negative (edge case)
        if ctx.current_equity <= Decimal::ZERO {
            return ValidationResult::Approve;
        }

        // Calculate total exposure after this trade
        let current_position_qty = ctx.get_current_position_qty();
        let total_qty = current_position_qty + ctx.proposal.quantity;
        let total_exposure = total_qty * ctx.proposal.price;

        // Calculate adjusted limit based on sentiment
        let adjusted_max_pct = self.calculate_adjusted_limit(ctx, ctx.proposal.side);

        // Calculate position percentage
        let position_pct = (total_exposure / ctx.current_equity)
            .to_f64()
            .unwrap_or(0.0);

        if position_pct > adjusted_max_pct {
            return ValidationResult::Reject(format!(
                "Position size ({:.2}%) exceeds limit ({:.2}%) [Sentiment Adjusted]",
                position_pct * 100.0,
                adjusted_max_pct * 100.0
            ));
        }

        ValidationResult::Approve
    }

    fn priority(&self) -> u8 {
        10 // Early check - no point validating other rules if position is too large
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::state::RiskState;
    use crate::domain::sentiment::Sentiment;
    use crate::domain::trading::portfolio::{Portfolio, Position};
    use crate::domain::trading::types::{OrderType, TradeProposal};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn create_test_proposal(symbol: &str, side: OrderSide, price: Decimal, qty: Decimal) -> TradeProposal {
        TradeProposal {
            symbol: symbol.to_string(),
            side,
            price,
            quantity: qty,
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_approve_small_position() {
        let validator = PositionSizeValidator::new(PositionSizeConfig {
            max_position_size_pct: 0.25, // 25% limit
        });

        let proposal = create_test_proposal("BTC/USD", OrderSide::Buy, dec!(50000), dec!(0.1));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // $100k equity
            &prices,
            &risk_state,
            None,
            None,
            None,
        );

        // Exposure: 0.1 * $50k = $5k = 5% of equity (well under 25% limit)
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_reject_oversized_position() {
        let validator = PositionSizeValidator::new(PositionSizeConfig {
            max_position_size_pct: 0.10, // 10% limit
        });

        let proposal = create_test_proposal("BTC/USD", OrderSide::Buy, dec!(50000), dec!(1.0));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // $100k equity
            &prices,
            &risk_state,
            None,
            None,
            None,
        );

        // Exposure: 1.0 * $50k = $50k = 50% of equity (exceeds 10% limit)
        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("50.00%"));
        assert!(result.rejection_reason().unwrap().contains("10.00%"));
    }

    #[tokio::test]
    async fn test_approve_sell_orders() {
        let validator = PositionSizeValidator::new(PositionSizeConfig::default());

        let proposal = create_test_proposal("BTC/USD", OrderSide::Sell, dec!(50000), dec!(10.0));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
            None,
        );

        // Sell orders should always be approved (they reduce exposure)
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_sentiment_adjustment_extreme_fear() {
        let validator = PositionSizeValidator::new(PositionSizeConfig {
            max_position_size_pct: 0.20, // 20% base limit
        });

        let proposal = create_test_proposal("BTC/USD", OrderSide::Buy, dec!(50000), dec!(0.25));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        // Extreme Fear sentiment (value = 10)
        let sentiment = Sentiment {
            value: 10,
            classification: SentimentClassification::ExtremeFear,
            timestamp: chrono::Utc::now(),
            source: "test".to_string(),
        };

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // $100k equity
            &prices,
            &risk_state,
            Some(&sentiment),
            None,
            None,
        );

        // Exposure: 0.25 * $50k = $12.5k = 12.5% of equity
        // Base limit: 20%, but with Extreme Fear adjustment: 20% * 0.5 = 10%
        // 12.5% > 10%, so should be rejected
        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("Sentiment Adjusted"));
    }

    #[tokio::test]
    async fn test_sentiment_no_adjustment_for_greed() {
        let validator = PositionSizeValidator::new(PositionSizeConfig {
            max_position_size_pct: 0.20, // 20% limit
        });

        let proposal = create_test_proposal("BTC/USD", OrderSide::Buy, dec!(50000), dec!(0.3));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        // Extreme Greed sentiment (no adjustment applied)
        let sentiment = Sentiment {
            value: 90,
            classification: SentimentClassification::ExtremeGreed,
            timestamp: chrono::Utc::now(),
            source: "test".to_string(),
        };

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // $100k equity
            &prices,
            &risk_state,
            Some(&sentiment),
            None,
            None,
        );

        // Exposure: 0.3 * $50k = $15k = 15% of equity
        // No sentiment adjustment for Greed, so base 20% limit applies
        // 15% < 20%, so should be approved
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_accumulation_with_existing_position() {
        let validator = PositionSizeValidator::new(PositionSizeConfig {
            max_position_size_pct: 0.15, // 15% limit
        });

        let proposal = create_test_proposal("BTC/USD", OrderSide::Buy, dec!(50000), dec!(0.1));
        
        let mut portfolio = Portfolio::new();
        portfolio.positions.insert(
            "BTC/USD".to_string(),
            Position {
                symbol: "BTC/USD".to_string(),
                quantity: dec!(0.1), // Already own 0.1 BTC
                average_price: dec!(48000),
            },
        );

        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // $100k equity
            &prices,
            &risk_state,
            None,
            None,
            None,
        );

        // Total position: 0.1 (existing) + 0.1 (new) = 0.2 BTC
        // Total exposure: 0.2 * $50k = $10k = 10% of equity
        // 10% < 15% limit, so should be approved
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_reject_accumulation_exceeding_limit() {
        let validator = PositionSizeValidator::new(PositionSizeConfig {
            max_position_size_pct: 0.10, // 10% limit
        });

        let proposal = create_test_proposal("BTC/USD", OrderSide::Buy, dec!(50000), dec!(0.15));
        
        let mut portfolio = Portfolio::new();
        portfolio.positions.insert(
            "BTC/USD".to_string(),
            Position {
                symbol: "BTC/USD".to_string(),
                quantity: dec!(0.1), // Already own 0.1 BTC
                average_price: dec!(48000),
            },
        );

        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // $100k equity
            &prices,
            &risk_state,
            None,
            None,
            None,
        );

        // Total position: 0.1 (existing) + 0.15 (new) = 0.25 BTC
        // Total exposure: 0.25 * $50k = $12.5k = 12.5% of equity
        // 12.5% > 10% limit, so should be rejected
        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
    }
}
