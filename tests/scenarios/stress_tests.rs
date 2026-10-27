//! Stress tests: adverse market and infrastructure scenarios.
//!
//! Covers the safety mechanisms that must hold when things go wrong: infrastructure
//! circuit breaker (open, recovery), daily loss and drawdown limits, consecutive losses,
//! and fail-closed behavior when a loss cannot even be computed (extreme values).

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rustrade::domain::risk::filters::circuit_breaker_validator::{
    CircuitBreakerConfig, CircuitBreakerValidator,
};
use rustrade::domain::risk::filters::{RiskValidator, ValidationContext, ValidationResult};
use rustrade::domain::risk::state::RiskState;
use rustrade::domain::trading::portfolio::Portfolio;
use rustrade::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rustrade::infrastructure::core::circuit_breaker::{
    CircuitBreaker, CircuitBreakerError, CircuitState,
};
use std::collections::HashMap;
use std::time::Duration;

fn proposal() -> TradeProposal {
    TradeProposal {
        symbol: "BTC/USD".to_string(),
        side: OrderSide::Buy,
        price: dec!(50000),
        quantity: dec!(0.1),
        order_type: OrderType::Market,
        reason: "stress test".to_string(),
        timestamp: 0,
        stop_loss: None,
        take_profit: None,
        correlation_id: None,
    }
}

fn validator() -> CircuitBreakerValidator {
    CircuitBreakerValidator::new(CircuitBreakerConfig {
        max_daily_loss_pct: dec!(0.05),
        max_drawdown_pct: dec!(0.10),
        consecutive_loss_limit: 5,
    })
}

/// Validates the standard proposal against `risk_state` at `current_equity`.
async fn validate(risk_state: &RiskState, current_equity: Decimal) -> ValidationResult {
    let proposal = proposal();
    let portfolio = Portfolio::new();
    let prices: HashMap<String, Decimal> = HashMap::new();
    let ctx = ValidationContext::new(
        &proposal,
        &portfolio,
        current_equity,
        &prices,
        risk_state,
        None,
        None,
        None,
        dec!(0),
        dec!(50_000),
        None,
    );
    validator().validate(&ctx).await
}

fn rejection(result: &ValidationResult) -> &str {
    match result {
        ValidationResult::Reject(reason) => reason,
        other => panic!("Expected Reject, got {:?}", other),
    }
}

/// Infrastructure circuit breaker opens after N consecutive failures.
#[tokio::test]
async fn test_stress_circuit_breaker_opens_after_failures() {
    let cb = CircuitBreaker::new("stress-test", 3, 2, Duration::from_secs(60));

    for _ in 0..3 {
        let r = cb.call(async { Result::<(), ()>::Err(()) }).await;
        assert!(matches!(r, Err(CircuitBreakerError::Inner(()))));
    }

    assert_eq!(cb.state().await, CircuitState::Open);

    let r = cb.call(async { Result::<(), ()>::Err(()) }).await;
    assert!(matches!(r, Err(CircuitBreakerError::Open(_))));
}

/// After the timeout, the breaker lets calls through again and closes after enough successes.
#[tokio::test]
async fn test_stress_circuit_breaker_recovers_after_timeout() {
    let cb = CircuitBreaker::new("stress-recovery", 2, 2, Duration::from_millis(50));

    for _ in 0..2 {
        let _ = cb.call(async { Result::<(), ()>::Err(()) }).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    tokio::time::sleep(Duration::from_millis(80)).await;

    for _ in 0..2 {
        let r = cb.call(async { Result::<(), ()>::Ok(()) }).await;
        assert!(r.is_ok(), "half-open breaker must let probe calls through");
    }
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// A failure during recovery reopens the breaker immediately.
#[tokio::test]
async fn test_stress_circuit_breaker_reopens_on_failed_probe() {
    let cb = CircuitBreaker::new("stress-probe", 2, 2, Duration::from_millis(50));

    for _ in 0..2 {
        let _ = cb.call(async { Result::<(), ()>::Err(()) }).await;
    }
    tokio::time::sleep(Duration::from_millis(80)).await;

    let _ = cb.call(async { Result::<(), ()>::Err(()) }).await;
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Flash crash: a 7% intraday loss breaches the 5% daily limit.
#[tokio::test]
async fn test_stress_daily_loss_breach_rejects_proposal() {
    let risk_state = RiskState {
        session_start_equity: dec!(100_000),
        ..Default::default()
    };
    let result = validate(&risk_state, dec!(93_000)).await;
    assert!(rejection(&result).contains("Daily loss"));
}

/// Slow bleed: within the daily limit, but 12% below the high water mark (limit 10%).
#[tokio::test]
async fn test_stress_drawdown_breach_rejects_proposal() {
    let risk_state = RiskState {
        session_start_equity: dec!(90_000),
        equity_high_water_mark: dec!(100_000),
        ..Default::default()
    };
    let result = validate(&risk_state, dec!(88_000)).await;
    assert!(rejection(&result).contains("drawdown"));
}

/// Losing streak: the consecutive loss limit stops trading regardless of equity.
#[tokio::test]
async fn test_stress_consecutive_losses_reject_proposal() {
    let risk_state = RiskState {
        session_start_equity: dec!(100_000),
        equity_high_water_mark: dec!(100_000),
        consecutive_losses: 5,
        ..Default::default()
    };
    let result = validate(&risk_state, dec!(100_000)).await;
    assert!(rejection(&result).contains("Consecutive loss"));
}

/// Corrupted or extreme equity values must reject, never pass as "no loss" or panic.
#[tokio::test]
async fn test_stress_uncomputable_loss_fails_closed() {
    let risk_state = RiskState {
        session_start_equity: dec!(0.0000000000000000000000000001),
        ..Default::default()
    };
    let result = validate(&risk_state, Decimal::MIN).await;
    assert!(rejection(&result).contains("cannot be evaluated"));
}

/// Normal conditions still approve: the stress safeguards are not over-triggering.
#[tokio::test]
async fn test_stress_normal_conditions_approve() {
    let risk_state = RiskState {
        session_start_equity: dec!(100_000),
        equity_high_water_mark: dec!(101_000),
        consecutive_losses: 1,
        ..Default::default()
    };
    let result = validate(&risk_state, dec!(99_500)).await;
    assert!(
        matches!(result, ValidationResult::Approve),
        "expected Approve, got {:?}",
        result
    );
}
