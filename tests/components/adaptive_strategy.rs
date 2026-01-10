use rustrade::application::agents::analyst::AnalystConfig;
use rustrade::application::strategies::strategy_selector::StrategySelector;
use rustrade::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use rustrade::domain::market::strategy_config::StrategyMode;

/// Test that StrategySelector correctly maps market regimes to appropriate strategies
#[test]
fn test_strategy_selector_ranging_to_vwap() {
    let config = AnalystConfig::default();

    // Create a Ranging regime
    let ranging_regime = MarketRegime::new(
        MarketRegimeType::Ranging,
        0.8,  // High confidence
        1.0,  // Low volatility
        10.0, // Low trend strength
    );

    // Start with Standard strategy
    let current_mode = StrategyMode::Standard;

    // Select strategy based on regime
    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should switch to VWAP for Ranging regime (v0.60 enhancement)
    assert_eq!(
        new_mode,
        StrategyMode::VWAP,
        "Should select VWAP strategy for Ranging regime"
    );
}

#[test]
fn test_strategy_selector_trending_up_to_trend_riding() {
    let config = AnalystConfig::default();

    // Create a TrendingUp regime
    let trending_regime = MarketRegime::new(
        MarketRegimeType::TrendingUp,
        0.9,  // High confidence
        1.5,  // Moderate volatility
        35.0, // High trend strength
    );

    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&trending_regime, &config, current_mode);

    // Should switch to TrendRiding for trending markets
    assert_eq!(
        new_mode,
        StrategyMode::TrendRiding,
        "Should select TrendRiding strategy for TrendingUp regime"
    );
}

#[test]
fn test_strategy_selector_trending_down_to_trend_riding() {
    let config = AnalystConfig::default();

    let trending_regime = MarketRegime::new(MarketRegimeType::TrendingDown, 0.85, 2.0, 40.0);

    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&trending_regime, &config, current_mode);

    assert_eq!(
        new_mode,
        StrategyMode::TrendRiding,
        "Should select TrendRiding strategy for TrendingDown regime"
    );
}

#[test]
fn test_strategy_selector_volatile_to_momentum() {
    let config = AnalystConfig::default();

    let volatile_regime = MarketRegime::new(
        MarketRegimeType::Volatile,
        0.7,
        5.0, // High volatility
        15.0,
    );

    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&volatile_regime, &config, current_mode);

    // Volatile markets should use Momentum (v0.60 enhancement - divergence detection)
    assert_eq!(
        new_mode,
        StrategyMode::Momentum,
        "Should select Momentum strategy for Volatile regime"
    );
}

#[test]
fn test_strategy_selector_unknown_to_standard() {
    let config = AnalystConfig::default();

    let unknown_regime = MarketRegime::unknown();

    let current_mode = StrategyMode::TrendRiding;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&unknown_regime, &config, current_mode);

    // Unknown regime should fallback to Standard
    assert_eq!(
        new_mode,
        StrategyMode::Standard,
        "Should select Standard strategy for Unknown regime"
    );
}

#[test]
fn test_strategy_selector_no_change_when_same() {
    let config = AnalystConfig::default();

    let ranging_regime = MarketRegime::new(MarketRegimeType::Ranging, 0.8, 1.0, 10.0);

    // Already using VWAP (which is correct for Ranging)
    let current_mode = StrategyMode::VWAP;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should stay with VWAP
    assert_eq!(
        new_mode,
        StrategyMode::VWAP,
        "Should keep VWAP strategy when already appropriate for Ranging"
    );
}
