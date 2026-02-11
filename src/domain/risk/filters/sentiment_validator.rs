use async_trait::async_trait;

use crate::domain::risk::filters::validator_trait::{
    RiskValidator, ValidationContext, ValidationResult,
};
use crate::domain::sentiment::SentimentClassification;
use crate::domain::trading::types::OrderSide;

/// Configuration for sentiment-based validation
#[derive(Debug, Clone, Default)]
pub struct SentimentConfig {
    /// Whether to block buys during Extreme Fear
    pub block_buys_on_extreme_fear: bool,

    /// Minimum sentiment score required to open Long positions (0-100)
    /// 0 (default) means no minimum score required
    pub min_score_for_longs: u8,
}

/// Validates trades based on market sentiment
///
/// This validator can enforce strict rules like "No Buys during Extreme Fear"
/// or requires a minimum sentiment score for bullish trades.
///
/// Note: Position sizing adjustments based on sentiment are handled by
/// the PositionSizeValidator, not here. This validator is for binary Block/Allow decisions.
pub struct SentimentValidator {
    config: SentimentConfig,
}

impl SentimentValidator {
    pub fn new(config: SentimentConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl RiskValidator for SentimentValidator {
    fn name(&self) -> &str {
        "SentimentValidator"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        // If no sentiment data, we can't validate, so approve
        let sentiment = match ctx.current_sentiment {
            Some(s) => s,
            None => return ValidationResult::Approve,
        };

        // Only validate Buy orders
        if !matches!(ctx.proposal.side, OrderSide::Buy) {
            return ValidationResult::Approve;
        }

        // Rule 1: Block Buys on Extreme Fear (if enabled)
        if self.config.block_buys_on_extreme_fear
            && sentiment.classification == SentimentClassification::ExtremeFear
        {
            return ValidationResult::Reject(format!(
                "Market Sentiment is Extreme Fear ({}) - logic blocked via config",
                sentiment.value
            ));
        }

        // Rule 2: Minimum Score for Longs
        if sentiment.value < self.config.min_score_for_longs {
            return ValidationResult::Reject(format!(
                "Market Sentiment Score {} is below minimum required for longs ({})",
                sentiment.value, self.config.min_score_for_longs
            ));
        }

        ValidationResult::Approve
    }

    fn priority(&self) -> u8 {
        40 // Low priority (after hard risk limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::state::RiskState;
    use crate::domain::sentiment::Sentiment;
    use crate::domain::trading::portfolio::Portfolio;
    use crate::domain::trading::types::{OrderType, TradeProposal};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn create_test_proposal() -> TradeProposal {
        TradeProposal {
            symbol: "BTC/USD".to_string(),
            side: OrderSide::Buy,
            price: dec!(50000),
            quantity: dec!(1),
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
            stop_loss: None,
            take_profit: None,
        }
    }

    fn create_context<'a>(
        proposal: &'a TradeProposal,
        portfolio: &'a Portfolio,
        risk_state: &'a RiskState,
        sentiment: Option<&'a Sentiment>,
        prices: &'a HashMap<String, Decimal>,
    ) -> ValidationContext<'a> {
        ValidationContext::new(
            proposal,
            portfolio,
            dec!(100000),
            prices,
            risk_state,
            sentiment,
            None,
            None,
            Decimal::ZERO,
            dec!(100000),
            None, // recent_candles
        )
    }

    #[tokio::test]
    async fn test_approve_no_sentiment() {
        let validator = SentimentValidator::new(SentimentConfig::default());
        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let risk_state = RiskState::default();
        let prices = HashMap::new();

        let ctx = create_context(&proposal, &portfolio, &risk_state, None, &prices);

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_block_buys_on_extreme_fear() {
        let validator = SentimentValidator::new(SentimentConfig {
            block_buys_on_extreme_fear: true,
            ..Default::default()
        });

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let risk_state = RiskState::default();
        let prices = HashMap::new();

        let sentiment = Sentiment {
            value: 10,
            classification: SentimentClassification::ExtremeFear,
            timestamp: chrono::Utc::now(),
            source: "test".to_string(),
        };

        let ctx = create_context(
            &proposal,
            &portfolio,
            &risk_state,
            Some(&sentiment),
            &prices,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("Extreme Fear"));
    }

    #[tokio::test]
    async fn test_allow_buys_on_extreme_fear_default() {
        let validator = SentimentValidator::new(SentimentConfig::default()); // block_buys = false

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let risk_state = RiskState::default();
        let prices = HashMap::new();

        let sentiment = Sentiment {
            value: 10,
            classification: SentimentClassification::ExtremeFear,
            timestamp: chrono::Utc::now(),
            source: "test".to_string(),
        };

        let ctx = create_context(
            &proposal,
            &portfolio,
            &risk_state,
            Some(&sentiment),
            &prices,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_reject_below_min_score() {
        let validator = SentimentValidator::new(SentimentConfig {
            min_score_for_longs: 30,
            ..Default::default()
        });

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let risk_state = RiskState::default();
        let prices = HashMap::new();

        let sentiment = Sentiment {
            value: 20, // Below 30
            classification: SentimentClassification::ExtremeFear,
            timestamp: chrono::Utc::now(),
            source: "test".to_string(),
        };

        let ctx = create_context(
            &proposal,
            &portfolio,
            &risk_state,
            Some(&sentiment),
            &prices,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("below minimum"));
    }
}
