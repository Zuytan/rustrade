use tracing::{debug, warn};

use crate::domain::risk::filters::validator_trait::{
    RiskValidator, ValidationContext, ValidationResult,
};

/// Orchestrates the execution of multiple risk validators
/// 
/// The pipeline executes validators in a specific order (defined by their priority).
/// If any validator returns a Rejection, the pipeline stops immediately and returns
/// that rejection. This implements a "Fail Fast" strategy.
pub struct RiskValidationPipeline {
    validators: Vec<Box<dyn RiskValidator>>,
}

impl RiskValidationPipeline {
    /// Create a new pipeline with the given validators.
    /// Validators are automatically sorted by priority (lower priority executes first).
    pub fn new(validators: Vec<Box<dyn RiskValidator>>) -> Self {
        let mut sorted_validators = validators;
        sorted_validators.sort_by_key(|v| v.priority());
        
        Self {
            validators: sorted_validators,
        }
    }

    /// Add a validator to the existing pipeline and resort
    pub fn add_validator(&mut self, validator: Box<dyn RiskValidator>) {
        self.validators.push(validator);
        self.validators.sort_by_key(|v| v.priority());
    }

    /// Execute all enabled validators in order
    pub async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        debug!(
            "Starting validation pipeline for {} {} (Val: {:.2})",
            ctx.proposal.side, ctx.proposal.symbol, ctx.calculate_proposal_exposure()
        );

        for validator in &self.validators {
            if !validator.is_enabled() {
                debug!("Skipping disabled validator: {}", validator.name());
                continue;
            }

            match validator.validate(ctx).await {
                ValidationResult::Reject(reason) => {
                    warn!(
                        "Validation failed at step {}: {}",
                        validator.name(),
                        reason
                    );
                    return ValidationResult::Reject(reason);
                }
                ValidationResult::Approve => {
                    debug!("Validator passed: {}", validator.name());
                    continue;
                }
            }
        }

        debug!("All validators passed");
        ValidationResult::Approve
    }
    
    /// Get list of active validator names (for introspection/API)
    pub fn list_active_validators(&self) -> Vec<&str> {
        self.validators
            .iter()
            .filter(|v| v.is_enabled())
            .map(|v| v.name())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::domain::risk::state::RiskState;
    use crate::domain::trading::portfolio::Portfolio;
    use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};

    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    // Mock validators for testing
    struct MockValidator {
        name: String,
        should_pass: bool,
        priority: u8,
    }

    #[async_trait]
    impl RiskValidator for MockValidator {
        fn name(&self) -> &str {
            &self.name
        }

        async fn validate(&self, _ctx: &ValidationContext<'_>) -> ValidationResult {
            if self.should_pass {
                ValidationResult::Approve
            } else {
                ValidationResult::Reject(format!("Rejected by {}", self.name))
            }
        }
        
        fn priority(&self) -> u8 {
            self.priority
        }
    }

    fn create_context<'a>() -> ValidationContext<'a> {
        // Dummy values, mocked validators don't use them
        // But we need valid references with sufficiently long lifetimes
        // We use Box::leak to fake 'static (or long enough) lifetime for the REFERENCED objects
        
        let proposal = Box::leak(Box::new(TradeProposal {
             symbol: "TEST".to_string(),
             side: OrderSide::Buy,
             price: dec!(100),
             quantity: dec!(1),
             order_type: OrderType::Market,
             reason: String::new(),
             timestamp: 0,
        }));
        
        let portfolio = Box::leak(Box::new(Portfolio::new()));
        let prices = Box::leak(Box::new(HashMap::new()));
        let risk_state = Box::leak(Box::new(RiskState::default()));
        
        ValidationContext {
             proposal,
             portfolio,
             current_equity: dec!(10000),
             current_prices: prices,
             risk_state,
             current_sentiment: None,
             correlation_matrix: None,
        }
    }
    
    #[tokio::test]
    async fn test_pipeline_execution_order() {
        let v1 = MockValidator { name: "V1".to_string(), should_pass: true, priority: 10 };
        let v2 = MockValidator { name: "V2".to_string(), should_pass: true, priority: 5 }; // Should run first
        
        let pipeline = RiskValidationPipeline::new(vec![Box::new(v1), Box::new(v2)]);
        
        // V2 has priority 5, V1 has 10. V2 should be first in the list
        let names = pipeline.list_active_validators();
        assert_eq!(names[0], "V2");
        assert_eq!(names[1], "V1");
        
        let ctx = create_context();
        let result = pipeline.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_pipeline_fail_fast() {
        let v1 = MockValidator { name: "V1".to_string(), should_pass: true, priority: 5 };
        let v2 = MockValidator { name: "V2".to_string(), should_pass: false, priority: 10 }; // Fails
        let v3 = MockValidator { name: "V3".to_string(), should_pass: true, priority: 15 }; // Shouldn't run
        
        let pipeline = RiskValidationPipeline::new(vec![
            Box::new(v1), Box::new(v2), Box::new(v3)
        ]);
        
        let ctx = create_context();
        let result = pipeline.validate(&ctx).await;
        assert!(result.is_rejected());
        assert_eq!(result.rejection_reason(), Some("Rejected by V2"));
    }
    
    #[tokio::test]
    async fn test_add_validator() {
        let mut pipeline = RiskValidationPipeline::new(vec![]);
        
        pipeline.add_validator(Box::new(MockValidator { 
            name: "HighPrio".to_string(), 
            should_pass: true, 
            priority: 100 
        }));
        
        pipeline.add_validator(Box::new(MockValidator { 
            name: "LowPrio".to_string(), 
            should_pass: true, 
            priority: 10 
        }));
        
        let names = pipeline.list_active_validators();
        assert_eq!(names, vec!["LowPrio", "HighPrio"]);
    }
}
