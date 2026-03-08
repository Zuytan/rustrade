use rust_decimal_macros::dec;

use rustrade::application::agents::analyst::AnalystConfig;
use rustrade::application::strategies::strategy_selector::StrategySelector;
use rustrade::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use rustrade::domain::market::strategy_config::StrategyMode;

/// Test that StrategySelector correctly maps market regimes to appropriate strategies
#[test]
fn test_strategy_selector_ranging_to_zscore() {
    let config = AnalystConfig::default();

    // Create a Ranging regime
    let ranging_regime = MarketRegime::new(
        MarketRegimeType::Ranging,
        dec!(0.8),  // High confidence
        dec!(2.0),  // High volatility
        dec!(10.0), // Low trend strength
    );

    // Start with RegimeAdaptive strategy
    let current_mode = StrategyMode::RegimeAdaptive;

    // Select strategy based on regime
    let new_mode = StrategySelector::select_best(ranging_regime, "BTCUSDT", &config, current_mode);

    // Should switch to ZScoreMR for Ranging regime (Modern Logic)
    assert_eq!(
        new_mode,
        StrategyMode::ZScoreMR,
        "Should select ZScoreMR strategy for Ranging regime"
    );
}

#[test]
fn test_strategy_selector_trending_to_regime_adaptive() {
    let config = AnalystConfig::default();

    // Create a TrendingUp regime
    let trending_regime = MarketRegime::new(
        MarketRegimeType::TrendingUp,
        dec!(0.9),  // High confidence
        dec!(1.5),  // Moderate volatility
        dec!(35.0), // High trend strength
    );

    let current_mode = StrategyMode::RegimeAdaptive;

    let new_mode = StrategySelector::select_best(trending_regime, "BTCUSDT", &config, current_mode);

    // Should stay/switch to RegimeAdaptive for trending markets
    assert_eq!(
        new_mode,
        StrategyMode::RegimeAdaptive,
        "Should select RegimeAdaptive strategy for TrendingUp regime"
    );
}

#[test]
fn test_strategy_selector_volatile_to_smc() {
    let config = AnalystConfig::default();

    let volatile_regime = MarketRegime::new(
        MarketRegimeType::Volatile,
        dec!(0.7),
        dec!(5.0), // High volatility
        dec!(15.0),
    );

    let current_mode = StrategyMode::RegimeAdaptive;

    let new_mode = StrategySelector::select_best(volatile_regime, "BTCUSDT", &config, current_mode);

    // Volatile markets should use SMC
    assert_eq!(
        new_mode,
        StrategyMode::SMC,
        "Should select SMC strategy for Volatile regime"
    );
}

#[test]
fn test_strategy_selector_unknown_to_regime_adaptive() {
    let config = AnalystConfig::default();

    let unknown_regime = MarketRegime::unknown();

    let current_mode = StrategyMode::RegimeAdaptive;

    let new_mode = StrategySelector::select_best(unknown_regime, "BTCUSDT", &config, current_mode);

    // Unknown regime defaults to RegimeAdaptive
    assert_eq!(
        new_mode,
        StrategyMode::RegimeAdaptive,
        "Should stay with RegimeAdaptive strategy for Unknown regime"
    );
}

#[test]
fn test_strategy_selector_no_change_when_same() {
    let config = AnalystConfig::default();

    // Low volatility ranging -> ZScoreMR
    let ranging_regime =
        MarketRegime::new(MarketRegimeType::Ranging, dec!(0.8), dec!(1.0), dec!(10.0));

    // Already using ZScoreMR
    let current_mode = StrategyMode::ZScoreMR;

    let new_mode = StrategySelector::select_best(ranging_regime, "BTCUSDT", &config, current_mode);

    // Should stay with ZScoreMR
    assert_eq!(
        new_mode,
        StrategyMode::ZScoreMR,
        "Should keep ZScoreMR when already appropriate for Ranging"
    );
}
