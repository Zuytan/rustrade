use rustrade::application::agents::analyst_config::AnalystConfig;
use rustrade::application::strategies::StrategyFactory;
use rustrade::domain::market::strategy_config::StrategyMode;

#[test]
fn test_snn_integration_instantiation() {
    let mut config = AnalystConfig::default();
    config.strategy.strategy_mode = StrategyMode::SnnSurrogate;

    // Create mock model to allow loading
    let hp = rustrade::domain::snn::hyperparams::SnnHyperparameters::default();
    let network =
        rustrade::domain::snn::competitive_network::CompetitiveSnnNetwork::new(10, 8, 8, 3, hp);
    let json = serde_json::to_string(&network).unwrap();
    let mut path = std::env::temp_dir();
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::thread::current().id().hash(&mut hasher);
    let thread_id_hash = hasher.finish();

    path.push(format!(
        "mock_snn_model_e2e_{}_{}.json",
        thread_id_hash,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    std::fs::write(&path, json).unwrap();

    config.strategy.snn_surrogate_model_path = path.to_str().unwrap().to_string();

    // Test factory creation
    let strategy = StrategyFactory::create(StrategyMode::SnnSurrogate, &config);
    assert_eq!(strategy.name(), "SnnSurrogate");

    // Test that the ensemble fallback doesn't trigger for SnnSurrogate
    let strategy_ens = StrategyFactory::create(StrategyMode::Ensemble, &config);
    assert_eq!(strategy_ens.name(), "Ensemble");

    // Clean up
    std::fs::remove_file(path).ok();
}
