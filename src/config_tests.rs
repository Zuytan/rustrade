use crate::config::Config;
use std::env;
use std::sync::Mutex;
use std::sync::OnceLock;

// Global lock to prevent race conditions when modifying environment variables in tests
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn get_env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn test_config_with_risk_score() {
    let _guard = get_env_lock().lock().unwrap();
    // Set up risk score
    env::set_var("RISK_APPETITE_SCORE", "7");

    let config = Config::from_env().unwrap();

    // Should have risk appetite set
    assert!(config.risk_appetite.is_some());
    let appetite = config.risk_appetite.unwrap();
    assert_eq!(appetite.score(), 7);

    // Should use calculated parameters
    let expected_risk_trade = appetite.calculate_risk_per_trade_percent();
    let expected_trailing_stop = appetite.calculate_trailing_stop_multiplier();
    let expected_rsi = appetite.calculate_rsi_threshold();
    let expected_max_position = appetite.calculate_max_position_size_pct();

    assert!((config.risk_per_trade_percent - expected_risk_trade).abs() < 0.0001);
    assert!((config.trailing_stop_atr_multiplier - expected_trailing_stop).abs() < 0.01);
    assert!((config.rsi_threshold - expected_rsi).abs() < 0.1);
    assert!((config.max_position_size_pct - expected_max_position).abs() < 0.001);

    // Cleanup
    env::remove_var("RISK_APPETITE_SCORE");
}

#[test]
fn test_config_without_risk_score() {
    let _guard = get_env_lock().lock().unwrap();
    // Remove RISK_APPETITE_SCORE if set
    env::remove_var("RISK_APPETITE_SCORE");

    // Set individual params
    env::set_var("RISK_PER_TRADE_PERCENT", "0.015");
    env::set_var("TRAILING_STOP_ATR_MULTIPLIER", "2.8");
    env::set_var("RSI_THRESHOLD", "60.0");
    env::set_var("MAX_POSITION_SIZE_PCT", "0.15");

    let config = Config::from_env().unwrap();

    // Should NOT have risk appetite set
    assert!(config.risk_appetite.is_none());

    // Should use individual env vars
    assert!((config.risk_per_trade_percent - 0.015).abs() < 0.0001);
    assert!((config.trailing_stop_atr_multiplier - 2.8).abs() < 0.01);
    assert!((config.rsi_threshold - 60.0).abs() < 0.1);
    assert!((config.max_position_size_pct - 0.15).abs() < 0.001);

    // Cleanup
    env::remove_var("RISK_PER_TRADE_PERCENT");
    env::remove_var("TRAILING_STOP_ATR_MULTIPLIER");
    env::remove_var("RSI_THRESHOLD");
    env::remove_var("MAX_POSITION_SIZE_PCT");
}

#[test]
fn test_config_risk_params_override() {
    let _guard = get_env_lock().lock().unwrap();
    // Set both risk score AND individual params
    env::set_var("RISK_APPETITE_SCORE", "9");
    env::set_var("RISK_PER_TRADE_PERCENT", "0.001"); // This should be ignored
    env::set_var("TRAILING_STOP_ATR_MULTIPLIER", "1.5"); // This should be ignored

    let config = Config::from_env().unwrap();

    // Risk score should override individual params
    assert!(config.risk_appetite.is_some());
    let appetite = config.risk_appetite.unwrap();

    // Should use calculated values, NOT env var values
    let expected_risk_trade = appetite.calculate_risk_per_trade_percent();
    assert!((config.risk_per_trade_percent - expected_risk_trade).abs() < 0.0001);
    assert!(config.risk_per_trade_percent > 0.02); // Score 9 should be aggressive, not 0.001

    // Cleanup
    env::remove_var("RISK_APPETITE_SCORE");
    env::remove_var("RISK_PER_TRADE_PERCENT");
    env::remove_var("TRAILING_STOP_ATR_MULTIPLIER");
}

#[test]
fn test_invalid_risk_score_returns_error() {
    let _guard = get_env_lock().lock().unwrap();
    env::set_var("RISK_APPETITE_SCORE", "15"); // Invalid score

    let result = Config::from_env();

    // Should fail with clear error message
    assert!(result.is_err());
    let err_msg = format!("{:?}", result.err().unwrap());
    assert!(err_msg.contains("must be between 1 and 10"));

    // Cleanup
    env::remove_var("RISK_APPETITE_SCORE");
}

#[test]
fn test_risk_score_boundary_values() {
    let _guard = get_env_lock().lock().unwrap();
    // Test minimum score
    env::set_var("RISK_APPETITE_SCORE", "1");
    let config = Config::from_env().unwrap();
    assert!(config.risk_appetite.is_some());
    assert_eq!(config.risk_appetite.unwrap().score(), 1);

    // Test maximum score
    env::set_var("RISK_APPETITE_SCORE", "10");
    let config = Config::from_env().unwrap();
    assert!(config.risk_appetite.is_some());
    assert_eq!(config.risk_appetite.unwrap().score(), 10);

    // Cleanup
    env::remove_var("RISK_APPETITE_SCORE");
}
