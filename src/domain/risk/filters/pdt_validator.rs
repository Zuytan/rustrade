use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::config::AssetClass;
use crate::domain::risk::filters::validator_trait::{RiskValidator, ValidationContext, ValidationResult};
use crate::domain::trading::types::OrderSide;

/// Configuration for PDT (Pattern Day Trader) protection
#[derive(Debug, Clone)]
pub struct PdtConfig {
    /// Whether PDT protection is enabled
    pub enabled: bool,
    
    /// Minimum equity threshold for PDT rules ($25,000 for US stocks)
    pub min_equity_threshold: Decimal,
    
    /// Maximum day trades allowed before restriction
    pub max_day_trades: u64,
    
    /// Asset class (PDT only applies to US stocks)
    pub asset_class: AssetClass,
}

impl Default for PdtConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_equity_threshold: Decimal::from(25000),
            max_day_trades: 3,
            asset_class: AssetClass::Stock,
        }
    }
}

/// Validates Pattern Day Trader (PDT) protection rules
/// 
/// PDT rules apply to US stock trading accounts with less than $25,000 equity.
/// If an account has made 3 or more day trades in a rolling 5-day period,
/// additional day trades are blocked to prevent PDT violations.
/// 
/// This validator blocks:
/// 1. Any BUY order if day trade count >= 3 (prevents opening new positions)
/// 2. Any SELL order that would complete a day trade if count >= 3
pub struct PdtValidator {
    config: PdtConfig,
}

impl PdtValidator {
    pub fn new(config: PdtConfig) -> Self {
        Self { config }
    }

    /// Check if PDT protection should be applied
    fn is_pdt_risk(&self, current_equity: Decimal) -> bool {
        current_equity < self.config.min_equity_threshold
    }

    /// Check if this sell would complete a day trade
    /// 
    /// Note: This is a simplified check. In a real system, we'd check if the
    /// position was opened today by examining the buy timestamp.
    fn is_closing_day_trade(&self, ctx: &ValidationContext<'_>) -> bool {
        // If we have a position in this symbol, selling it could be a day trade
        ctx.portfolio.positions.contains_key(&ctx.proposal.symbol)
    }
}

#[async_trait]
impl RiskValidator for PdtValidator {
    fn name(&self) -> &str {
        "PdtValidator"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        // PDT protection only applies if enabled and for Stock asset class
        if !self.config.enabled || self.config.asset_class != AssetClass::Stock {
            return ValidationResult::Approve;
        }

        // Check if account is subject to PDT rules
        if !self.is_pdt_risk(ctx.current_equity) {
            return ValidationResult::Approve;
        }

        // Check if day trade limit has been reached
        if ctx.portfolio.day_trades_count < self.config.max_day_trades {
            return ValidationResult::Approve;
        }

        // Block BUY orders (prevents opening new positions that could be day traded)
        if matches!(ctx.proposal.side, OrderSide::Buy) {
            return ValidationResult::Reject(format!(
                "PDT PROTECT: Cannot open new position (Day trades: {}, Equity: {})",
                ctx.portfolio.day_trades_count,
                ctx.current_equity
            ));
        }

        // Block SELL orders that would complete a day trade
        if matches!(ctx.proposal.side, OrderSide::Sell) && self.is_closing_day_trade(ctx) {
            return ValidationResult::Reject(format!(
                "PDT PROTECT: Cannot complete day trade (Day trades: {}, Equity: {})",
                ctx.portfolio.day_trades_count,
                ctx.current_equity
            ));
        }

        ValidationResult::Approve
    }

    fn priority(&self) -> u8 {
        20 // Medium-high priority (after circuit breakers, before position size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::state::RiskState;
    use crate::domain::trading::portfolio::{Portfolio, Position};
    use crate::domain::trading::types::{OrderType, TradeProposal};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn create_test_proposal(side: OrderSide) -> TradeProposal {
        TradeProposal {
            symbol: "AAPL".to_string(),
            side,
            price: dec!(150),
            quantity: dec!(10),
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_approve_high_equity_account() {
        let validator = PdtValidator::new(PdtConfig::default());

        let proposal = create_test_proposal(OrderSide::Buy);
        let mut portfolio = Portfolio::new();
        portfolio.day_trades_count = 5; // Exceeded limit
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(30000), // Above $25k threshold
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should approve because equity > $25k (not subject to PDT)
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_approve_below_day_trade_limit() {
        let validator = PdtValidator::new(PdtConfig::default());

        let proposal = create_test_proposal(OrderSide::Buy);
        let mut portfolio = Portfolio::new();
        portfolio.day_trades_count = 2; // Below limit of 3
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(20000), // Below $25k threshold
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should approve because day_trades_count < 3
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_reject_buy_when_pdt_saturated() {
        let validator = PdtValidator::new(PdtConfig::default());

        let proposal = create_test_proposal(OrderSide::Buy);
        let mut portfolio = Portfolio::new();
        portfolio.day_trades_count = 3; // At limit
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(20000), // Below $25k threshold
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should reject BUY when PDT saturated
        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("PDT PROTECT"));
        assert!(result.rejection_reason().unwrap().contains("Cannot open new position"));
    }

    #[tokio::test]
    async fn test_reject_sell_completing_day_trade() {
        let validator = PdtValidator::new(PdtConfig::default());

        let proposal = create_test_proposal(OrderSide::Sell);
        let mut portfolio = Portfolio::new();
        portfolio.day_trades_count = 3; // At limit
        
        // Add existing position (indicates we bought today, so selling = day trade)
        portfolio.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: dec!(10),
                average_price: dec!(145),
            },
        );
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(20000), // Below $25k threshold
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should reject SELL that completes a day trade
        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("PDT PROTECT"));
        assert!(result.rejection_reason().unwrap().contains("Cannot complete day trade"));
    }

    #[tokio::test]
    async fn test_approve_sell_no_position() {
        let validator = PdtValidator::new(PdtConfig::default());

        let proposal = create_test_proposal(OrderSide::Sell);
        let mut portfolio = Portfolio::new();
        portfolio.day_trades_count = 3; // At limit
        // No position in AAPL
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(20000), // Below $25k threshold
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should approve SELL if no position exists (can't be a day trade)
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_disabled_pdt_protection() {
        let validator = PdtValidator::new(PdtConfig {
            enabled: false,
            ..Default::default()
        });

        let proposal = create_test_proposal(OrderSide::Buy);
        let mut portfolio = Portfolio::new();
        portfolio.day_trades_count = 5; // Way over limit
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(10000), // Well below $25k
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should approve because PDT protection is disabled
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_crypto_not_subject_to_pdt() {
        let validator = PdtValidator::new(PdtConfig {
            asset_class: AssetClass::Crypto,
            ..Default::default()
        });

        let proposal = create_test_proposal(OrderSide::Buy);
        let mut portfolio = Portfolio::new();
        portfolio.day_trades_count = 10; // Way over limit
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(5000), // Low equity
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should approve because Crypto is not subject to PDT rules
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }
}
