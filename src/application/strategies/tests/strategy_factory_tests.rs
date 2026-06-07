use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::strategies::strategy_factory::StrategyFactory;
use crate::domain::market::strategy_config::StrategyMode;
use std::fs;
use std::path::PathBuf;

fn create_mock_model(name: &str) -> PathBuf {
    let hp = crate::domain::snn::hyperparams::SnnHyperparameters::default();
    let network =
        crate::domain::snn::competitive_network::CompetitiveSnnNetwork::new(10, 8, 8, 3, hp);
    let json = serde_json::to_string(&network).unwrap();
    let mut path = std::env::temp_dir();

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::thread::current().id().hash(&mut hasher);
    let thread_id_hash = hasher.finish();

    path.push(format!(
        "mock_snn_model_factory_{}_{}_{}.json",
        name,
        thread_id_hash,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    fs::write(&path, json).unwrap();
    path
}

#[test]
fn test_factory_ml_mode_falls_back_to_ensemble() {
    let config = AnalystConfig::default();
    let strategy = StrategyFactory::create(StrategyMode::ML, &config);
    // Since ML mode falls back to Ensemble, the strategy name should be "Ensemble"
    assert_eq!(strategy.name(), "Ensemble");
}

#[test]
fn test_factory_snn_surrogate_mode_creates_snn() {
    let model_path = create_mock_model("valid");
    let mut config = AnalystConfig::default();
    config.strategy.snn_surrogate_model_path = model_path.to_str().unwrap().to_string();

    let strategy = StrategyFactory::create(StrategyMode::SnnSurrogate, &config);
    assert_eq!(strategy.name(), "SnnSurrogate");

    fs::remove_file(model_path).ok();
}

#[test]
fn test_factory_snn_surrogate_fallback_on_bad_model() {
    let mut config = AnalystConfig::default();
    config.strategy.snn_surrogate_model_path = "non_existent_model_file_12345.json".to_string();

    let strategy = StrategyFactory::create(StrategyMode::SnnSurrogate, &config);
    assert_eq!(strategy.name(), "Ensemble"); // Should fall back
}

#[test]
fn test_factory_all_modes_compile() {
    let config = AnalystConfig::default();
    // Just verify that instantiating every strategy mode doesn't panic
    let modes = vec![
        StrategyMode::RegimeAdaptive,
        StrategyMode::SMC,
        StrategyMode::Ensemble,
        StrategyMode::ZScoreMR,
        StrategyMode::StatMomentum,
        StrategyMode::OrderFlow,
        StrategyMode::ML,
        StrategyMode::SnnSurrogate,
    ];

    for mode in modes {
        let _strategy = StrategyFactory::create(mode, &config);
    }
}
