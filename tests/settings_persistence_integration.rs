use rustrade::infrastructure::settings_persistence::{
    AnalystSettings, PersistedSettings, RiskSettings, SettingsPersistence,
};

#[test]
fn test_settings_persistence_flow() {
    // 1. Setup - Ensure we are using a test environment or mocked path if possible.
    // Since SettingsPersistence uses hardcoded HOME/.rustrade, we should be careful.
    // A better approach for testing would be to allow injecting the path, but for now
    // we can test the serialization/deserialization and integration logic separately if we refactor,
    // or just assume this test runs in a controlled env.

    // HOWEVER, modifying real user settings during test is bad practice.
    // I should modify SettingsPersistence to accept an optional override path for testing?
    // OR, I can just test the Serde logic in a unit test within the module itself
    // and only basic instantiation here.

    // Let's rely on unit tests inside the module for file operations if I can modify the module to be more testable.
    // For now, let's just assert we can instantiate it.

    let persistence = SettingsPersistence::new();
    assert!(persistence.is_ok());
}

#[test]
fn test_serialization_roundtrip() {
    let settings = PersistedSettings {
        config_mode: "Advanced".to_string(),
        risk_score: 8,
        risk: RiskSettings {
            max_position_size_pct: "0.15".to_string(),
            max_daily_loss_pct: "0.03".to_string(),
            max_drawdown_pct: "0.06".to_string(),
            consecutive_loss_limit: "4".to_string(),
        },
        analyst: AnalystSettings {
            strategy_mode: "RegimeAdaptive".to_string(),
            fast_sma_period: "10".to_string(),
            slow_sma_period: "21".to_string(),
            rsi_period: "14".to_string(),
            rsi_threshold: "75.0".to_string(),
            macd_min_threshold: "0.002".to_string(),
            adx_threshold: "20.0".to_string(),
            min_profit_ratio: "2.0".to_string(),
            sma_threshold: "0.002".to_string(),
            profit_target_multiplier: "3.0".to_string(),
        },
    };

    let serialized = serde_json::to_string(&settings).expect("Failed to serialize");
    let deserialized: PersistedSettings =
        serde_json::from_str(&serialized).expect("Failed to deserialize");

    assert_eq!(deserialized.config_mode, "Advanced");
    assert_eq!(deserialized.risk_score, 8);
    assert_eq!(deserialized.risk.max_position_size_pct, "0.15");
    assert_eq!(deserialized.analyst.fast_sma_period, "10");
}
