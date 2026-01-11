use rustrade::application::agents::analyst::AnalystConfig;
use rustrade::application::strategies::strategy_selector::StrategySelector;
use rustrade::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use rustrade::domain::market::strategy_config::StrategyMode;

/// Test that StrategySelector correctly maps market regimes to appropriate strategies
#[test]
fn test_strategy_selector_ranging_to_vwap() {
    let config = AnalystConfig::default();

    // Create a Ranging regime with HIGH volatility (>= 1.5)
    // This triggers VWAP instead of MeanReversion
    let ranging_regime = MarketRegime::new(
        MarketRegimeType::Ranging,
        0.8,  // High confidence
        2.0,  // High volatility -> VWAP
        10.0, // Low trend strength
    );

    // Start with Standard strategy
    let current_mode = StrategyMode::Standard;

    // Select strategy based on regime
    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should switch to VWAP for high-volatility Ranging regime
    assert_eq!(
        new_mode,
        StrategyMode::VWAP,
        "Should select VWAP strategy for high-volatility Ranging regime"
    );
}

#[test]
fn test_strategy_selector_ranging_low_vol_to_mean_reversion() {
    let config = AnalystConfig::default();

    // Create a Ranging regime with LOW volatility (< 1.5)
    let ranging_regime = MarketRegime::new(
        MarketRegimeType::Ranging,
        0.8,  // High confidence
        1.0,  // Low volatility -> MeanReversion
        10.0, // Low trend strength
    );

    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should switch to MeanReversion for low-volatility Ranging regime
    assert_eq!(
        new_mode,
        StrategyMode::MeanReversion,
        "Should select MeanReversion strategy for low-volatility Ranging regime"
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

    // Start with Standard (unknown regime has confidence 0, hysteresis kicks in)
    // But since current_mode IS Standard, it should stay Standard
    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&unknown_regime, &config, current_mode);

    // Unknown regime with confidence 0 and current mode Standard stays Standard
    assert_eq!(
        new_mode,
        StrategyMode::Standard,
        "Should stay with Standard strategy for Unknown regime"
    );
}

#[test]
fn test_strategy_selector_no_change_when_same() {
    let config = AnalystConfig::default();

    // Low volatility ranging -> MeanReversion
    let ranging_regime = MarketRegime::new(MarketRegimeType::Ranging, 0.8, 1.0, 10.0);

    // Already using MeanReversion (which is correct for low-vol Ranging)
    let current_mode = StrategyMode::MeanReversion;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should stay with MeanReversion
    assert_eq!(
        new_mode,
        StrategyMode::MeanReversion,
        "Should keep MeanReversion when already appropriate for low-vol Ranging"
    );
}
