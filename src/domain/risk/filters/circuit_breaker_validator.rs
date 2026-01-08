use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::domain::risk::filters::validator_trait::{RiskValidator, ValidationContext, ValidationResult};

/// Configuration for circuit breaker validation
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Maximum daily loss as percentage of starting equity (e.g., 0.02 = 2%)
    pub max_daily_loss_pct: f64,
    
    /// Maximum drawdown from high water mark as percentage (e.g., 0.10 = 10%)
    pub max_drawdown_pct: f64,
    
    /// Maximum consecutive losing trades before halt
    pub consecutive_loss_limit: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            max_daily_loss_pct: 0.02,    // 2%
            max_drawdown_pct: 0.05,      // 5%
            consecutive_loss_limit: 3,
        }
    }
}

/// Validates that circuit breaker conditions haven't been triggered
/// 
/// This validator implements three critical safety checks:
/// 1. Daily Loss Limit: Prevents excessive losses in a single trading session
/// 2. Drawdown Limit: Prevents portfolio from declining too much from peak
/// 3. Consecutive Loss Limit: Halts trading after too many losing trades in a row
/// 
/// If any of these limits are breached, all new trades are blocked until
/// manual intervention or automatic reset (e.g., new trading day).
pub struct CircuitBreakerValidator {
    config: CircuitBreakerConfig,
}

impl CircuitBreakerValidator {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self { config }
    }

    /// Check daily loss limit
    fn check_daily_loss(&self, ctx: &ValidationContext<'_>) -> Option<String> {
        if ctx.risk_state.session_start_equity > Decimal::ZERO {
            let daily_loss_pct = ((ctx.current_equity - ctx.risk_state.session_start_equity)
                / ctx.risk_state.session_start_equity)
                .to_f64()
                .unwrap_or(0.0);

            if daily_loss_pct < -self.config.max_daily_loss_pct {
                return Some(format!(
                    "Daily loss limit breached: {:.2}% (limit: {:.2}%) [Start: {}, Current: {}]",
                    daily_loss_pct * 100.0,
                    self.config.max_daily_loss_pct * 100.0,
                    ctx.risk_state.session_start_equity,
                    ctx.current_equity
                ));
            }
        }
        None
    }

    /// Check drawdown limit from high water mark
    fn check_drawdown(&self, ctx: &ValidationContext<'_>) -> Option<String> {
        if ctx.risk_state.equity_high_water_mark > Decimal::ZERO {
            let drawdown_pct = ((ctx.current_equity - ctx.risk_state.equity_high_water_mark)
                / ctx.risk_state.equity_high_water_mark)
                .to_f64()
                .unwrap_or(0.0);

            if drawdown_pct < -self.config.max_drawdown_pct {
                return Some(format!(
                    "Max drawdown breached: {:.2}% (limit: {:.2}%)",
                    drawdown_pct * 100.0,
                    self.config.max_drawdown_pct * 100.0
                ));
            }
        }
        None
    }

    /// Check consecutive losses limit
    fn check_consecutive_losses(&self, ctx: &ValidationContext<'_>) -> Option<String> {
        if ctx.risk_state.consecutive_losses >= self.config.consecutive_loss_limit {
            return Some(format!(
                "Consecutive loss limit reached: {} trades (limit: {})",
                ctx.risk_state.consecutive_losses,
                self.config.consecutive_loss_limit
            ));
        }
        None
    }
}

#[async_trait]
impl RiskValidator for CircuitBreakerValidator {
    fn name(&self) -> &str {
        "CircuitBreakerValidator"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        // Check all circuit breaker conditions
        if let Some(reason) = self.check_daily_loss(ctx) {
            return ValidationResult::Reject(reason);
        }

        if let Some(reason) = self.check_drawdown(ctx) {
            return ValidationResult::Reject(reason);
        }

        if let Some(reason) = self.check_consecutive_losses(ctx) {
            return ValidationResult::Reject(reason);
        }

        ValidationResult::Approve
    }

    fn priority(&self) -> u8 {
        1 // Highest priority - circuit breakers should be checked first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::state::RiskState;
    use crate::domain::trading::portfolio::Portfolio;
    use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn create_test_proposal() -> TradeProposal {
        TradeProposal {
            symbol: "BTC/USD".to_string(),
            side: OrderSide::Buy,
            price: dec!(50000),
            quantity: dec!(0.1),
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_approve_normal_conditions() {
        let validator = CircuitBreakerValidator::new(CircuitBreakerConfig::default());

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        
        let mut risk_state = RiskState::default();
        risk_state.session_start_equity = dec!(100000);
        risk_state.equity_high_water_mark = dec!(100000);
        risk_state.consecutive_losses = 0;

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // No loss
            &prices,
            &risk_state,
            None,
            None,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_reject_daily_loss_limit() {
        let validator = CircuitBreakerValidator::new(CircuitBreakerConfig {
            max_daily_loss_pct: 0.05, // 5% limit
            ..Default::default()
        });

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        
        let mut risk_state = RiskState::default();
        risk_state.session_start_equity = dec!(100000);
        risk_state.equity_high_water_mark = dec!(100000);

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(90000), // -10% loss (exceeds 5% limit)
            &prices,
            &risk_state,
            None,
            None,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("Daily loss limit breached"));
        assert!(result.rejection_reason().unwrap().contains("-10.00%"));
    }

    #[tokio::test]
    async fn test_reject_drawdown_limit() {
        let validator = CircuitBreakerValidator::new(CircuitBreakerConfig {
            max_drawdown_pct: 0.10, // 10% limit
            ..Default::default()
        });

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        
        let mut risk_state = RiskState::default();
        risk_state.session_start_equity = dec!(100000);
        risk_state.equity_high_water_mark = dec!(120000); // Peak was $120k

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000), // Current $100k = -16.67% from peak (exceeds 10% limit)
            &prices,
            &risk_state,
            None,
            None,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("Max drawdown breached"));
        assert!(result.rejection_reason().unwrap().contains("-16.67%"));
    }

    #[tokio::test]
    async fn test_reject_consecutive_losses() {
        let validator = CircuitBreakerValidator::new(CircuitBreakerConfig {
            consecutive_loss_limit: 3,
            ..Default::default()
        });

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        
        let mut risk_state = RiskState::default();
        risk_state.session_start_equity = dec!(100000);
        risk_state.equity_high_water_mark = dec!(100000);
        risk_state.consecutive_losses = 3; // Hit the limit

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(99000),
            &prices,
            &risk_state,
            None,
            None,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("Consecutive loss limit reached"));
        assert!(result.rejection_reason().unwrap().contains("3 trades"));
    }

    #[tokio::test]
    async fn test_approve_small_loss_within_limits() {
        let validator = CircuitBreakerValidator::new(CircuitBreakerConfig {
            max_daily_loss_pct: 0.05, // 5% limit
            max_drawdown_pct: 0.10,   // 10% limit
            consecutive_loss_limit: 3,
        });

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        
        let mut risk_state = RiskState::default();
        risk_state.session_start_equity = dec!(100000);
        risk_state.equity_high_water_mark = dec!(100000);
        risk_state.consecutive_losses = 2; // Below limit

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(98000), // -2% loss (within 5% limit)
            &prices,
            &risk_state,
            None,
            None,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_approve_profit_scenario() {
        let validator = CircuitBreakerValidator::new(CircuitBreakerConfig::default());

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        
        let mut risk_state = RiskState::default();
        risk_state.session_start_equity = dec!(100000);
        risk_state.equity_high_water_mark = dec!(100000);
        risk_state.consecutive_losses = 0;

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(110000), // +10% profit
            &prices,
            &risk_state,
            None,
            None,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_multiple_breaches_returns_first() {
        let validator = CircuitBreakerValidator::new(CircuitBreakerConfig {
            max_daily_loss_pct: 0.05,
            max_drawdown_pct: 0.10,
            consecutive_loss_limit: 2,
        });

        let proposal = create_test_proposal();
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        
        let mut risk_state = RiskState::default();
        risk_state.session_start_equity = dec!(100000);
        risk_state.equity_high_water_mark = dec!(100000);
        risk_state.consecutive_losses = 3; // Breaches consecutive loss limit

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(80000), // -20% (breaches both daily loss and drawdown)
            &prices,
            &risk_state,
            None,
            None,
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        // Should return daily loss breach first (checked first in code)
        assert!(result.rejection_reason().unwrap().contains("Daily loss limit breached"));
    }
}
