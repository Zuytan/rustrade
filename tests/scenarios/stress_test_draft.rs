//! Stress tests: circuit breaker and daily loss scenarios.
//! Run in CI to ensure resilience under adverse conditions.

use rust_decimal_macros::dec;
use rustrade::domain::risk::filters::circuit_breaker_validator::{
    CircuitBreakerConfig, CircuitBreakerValidator,
};
use rustrade::domain::risk::filters::{RiskValidator, ValidationContext, ValidationResult};
use rustrade::domain::risk::state::RiskState;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rustrade::infrastructure::core::circuit_breaker::{CircuitBreaker, CircuitBreakerError};
use std::collections::HashMap;
use std::time::Duration;

/// Infrastructure circuit breaker opens after N consecutive failures.
#[tokio::test]
async fn test_stress_circuit_breaker_opens_after_failures() {
    let cb = CircuitBreaker::new("stress-test", 3, 2, Duration::from_secs(60));

    for _ in 0..3 {
        let r = cb.call(async { Result::<(), ()>::Err(()) }).await;
        assert!(matches!(r, Err(CircuitBreakerError::Inner(()))));
    }

    let state = cb.state().await;
    assert_eq!(
        state,
        rustrade::infrastructure::core::circuit_breaker::CircuitState::Open
    );

    let r = cb.call(async { Result::<(), ()>::Err(()) }).await;
    assert!(matches!(r, Err(CircuitBreakerError::Open(_))));
}

/// CircuitBreakerValidator rejects proposals when daily loss limit is breached (flash-crash scenario).
#[tokio::test]
async fn test_stress_daily_loss_breach_rejects_proposal() {
    let validator = CircuitBreakerValidator::new(CircuitBreakerConfig {
        max_daily_loss_pct: dec!(0.05),
        max_drawdown_pct: dec!(0.10),
        consecutive_loss_limit: 5,
    });

    let proposal = TradeProposal {
        symbol: "BTC/USD".to_string(),
        side: OrderSide::Buy,
        price: dec!(50000),
        quantity: dec!(0.1),
        order_type: OrderType::Market,
        reason: "stress test".to_string(),
        timestamp: 0,
        stop_loss: None,
        take_profit: None,
    };

    let portfolio = Portfolio::new();
    let prices: HashMap<String, rust_decimal::Decimal> = HashMap::new();
    let risk_state = RiskState {
        session_start_equity: dec!(100_000),
        ..Default::default()
    };
    let current_equity = dec!(93_000); // 7% loss > 5% limit

    let ctx = ValidationContext::new(
        &proposal,
        &portfolio,
        current_equity,
        &prices,
        &risk_state,
        None,
        None,
        None,
        dec!(0),
        dec!(50_000),
        None,
    );

    let result = validator.validate(&ctx).await;
    match &result {
        ValidationResult::Reject(reason) => assert!(reason.contains("Daily loss")),
        _ => panic!("Expected Reject for daily loss breach, got {:?}", result),
    }
}
