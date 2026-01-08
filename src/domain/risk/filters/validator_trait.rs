use async_trait::async_trait;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::domain::risk::state::RiskState;
use crate::domain::sentiment::Sentiment;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::TradeProposal;

/// Result of a risk validation check
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    /// Validation passed, trade can proceed
    Approve,
    /// Validation failed, trade should be rejected with a reason
    Reject(String),
}

impl ValidationResult {
    /// Check if the result is approval
    pub fn is_approved(&self) -> bool {
        matches!(self, ValidationResult::Approve)
    }

    /// Check if the result is rejection
    pub fn is_rejected(&self) -> bool {
        matches!(self, ValidationResult::Reject(_))
    }

    /// Get rejection reason if rejected
    pub fn rejection_reason(&self) -> Option<&str> {
        match self {
            ValidationResult::Reject(reason) => Some(reason),
            ValidationResult::Approve => None,
        }
    }
}

/// Context shared across all validators during a validation run
/// 
/// This struct provides all the necessary information for validators to make decisions
/// without needing direct access to the RiskManager's internal state.
#[derive(Debug)]
pub struct ValidationContext<'a> {
    /// The trade proposal being validated
    pub proposal: &'a TradeProposal,
    
    /// Current portfolio state
    pub portfolio: &'a Portfolio,
    
    /// Current total equity (cash + positions value)
    pub current_equity: Decimal,
    
    /// Current market prices for all symbols
    pub current_prices: &'a HashMap<String, Decimal>,
    
    /// Current risk state (HWM, consecutive losses, etc.)
    pub risk_state: &'a RiskState,
    
    /// Current market sentiment (if available)
    pub current_sentiment: Option<&'a Sentiment>,
    
    /// Current correlation matrix (if available)
    pub correlation_matrix: Option<&'a HashMap<(String, String), f64>>,
    
    /// Current volatility multiplier (if available, e.g. from VolatilityManager)
    pub volatility_multiplier: Option<f64>,
}

impl<'a> ValidationContext<'a> {
    /// Create a new validation context
    pub fn new(
        proposal: &'a TradeProposal,
        portfolio: &'a Portfolio,
        current_equity: Decimal,
        current_prices: &'a HashMap<String, Decimal>,
        risk_state: &'a RiskState,
        current_sentiment: Option<&'a Sentiment>,
        correlation_matrix: Option<&'a HashMap<(String, String), f64>>,
        volatility_multiplier: Option<f64>,
    ) -> Self {
        Self {
            proposal,
            portfolio,
            current_equity,
            current_prices,
            risk_state,
            current_sentiment,
            correlation_matrix,
            volatility_multiplier,
        }
    }

    /// Get the current price for the proposal's symbol
    pub fn get_proposal_price(&self) -> Decimal {
        self.current_prices
            .get(&self.proposal.symbol)
            .copied()
            .unwrap_or(self.proposal.price)
    }

    /// Calculate the total exposure for the proposal
    pub fn calculate_proposal_exposure(&self) -> Decimal {
        self.proposal.price * self.proposal.quantity
    }

    /// Get current position quantity for the proposal's symbol
    pub fn get_current_position_qty(&self) -> Decimal {
        self.portfolio
            .positions
            .get(&self.proposal.symbol)
            .map(|p| p.quantity)
            .unwrap_or(Decimal::ZERO)
    }
}

/// Trait for all risk validators
/// 
/// Each validator implements a specific risk check (e.g., position size, circuit breaker).
/// Validators are executed in priority order by the ValidationPipeline.
#[async_trait]
pub trait RiskValidator: Send + Sync {
    /// Unique name for logging and debugging
    fn name(&self) -> &str;

    /// Perform validation check
    /// 
    /// Returns:
    /// - `ValidationResult::Approve` if the trade passes this validator's checks
    /// - `ValidationResult::Reject(reason)` if the trade should be blocked
    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult;

    /// Whether this validator is currently enabled
    /// 
    /// Disabled validators are skipped during pipeline execution.
    /// Default: true (always enabled)
    fn is_enabled(&self) -> bool {
        true
    }

    /// Priority order (lower = earlier execution)
    /// 
    /// Validators with lower priority values execute first.
    /// This allows critical checks (e.g., circuit breakers) to run before
    /// less critical ones (e.g., correlation filters).
    /// 
    /// Default: 100 (medium priority)
    fn priority(&self) -> u8 {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_validation_result_is_approved() {
        assert!(ValidationResult::Approve.is_approved());
        assert!(!ValidationResult::Reject("test".to_string()).is_approved());
    }

    #[test]
    fn test_validation_result_is_rejected() {
        assert!(!ValidationResult::Approve.is_rejected());
        assert!(ValidationResult::Reject("test".to_string()).is_rejected());
    }

    #[test]
    fn test_validation_result_rejection_reason() {
        assert_eq!(ValidationResult::Approve.rejection_reason(), None);
        assert_eq!(
            ValidationResult::Reject("insufficient funds".to_string()).rejection_reason(),
            Some("insufficient funds")
        );
    }

    #[test]
    fn test_validation_context_get_proposal_price() {
        use crate::domain::trading::types::{OrderSide, OrderType};
        
        let proposal = TradeProposal {
            symbol: "BTC/USD".to_string(),
            side: OrderSide::Buy,
            price: dec!(50000),
            quantity: dec!(1),
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        };

        let portfolio = Portfolio::new();
        let mut prices = HashMap::new();
        prices.insert("BTC/USD".to_string(), dec!(51000));
        
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
        );

        // Should return current market price, not proposal price
        assert_eq!(ctx.get_proposal_price(), dec!(51000));
    }

    #[test]
    fn test_validation_context_calculate_exposure() {
        use crate::domain::trading::types::{OrderSide, OrderType};
        
        let proposal = TradeProposal {
            symbol: "BTC/USD".to_string(),
            side: OrderSide::Buy,
            price: dec!(50000),
            quantity: dec!(2),
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        };

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
        );

        assert_eq!(ctx.calculate_proposal_exposure(), dec!(100000));
    }
}
