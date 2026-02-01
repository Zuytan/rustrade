use rust_decimal_macros::dec;

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
        dec!(0.8),  // High confidence
        dec!(2.0),  // High volatility -> VWAP
        dec!(10.0), // Low trend strength
    );

    // Start with Standard strategy
    let current_mode = StrategyMode::Standard;

    // Select strategy based on regime
    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should switch to ZScoreMR for Ranging regime (Modern Logic)
    assert_eq!(
        new_mode,
        StrategyMode::ZScoreMR,
        "Should select ZScoreMR strategy for Ranging regime"
    );
}

#[test]
fn test_strategy_selector_ranging_low_vol_to_mean_reversion() {
    let config = AnalystConfig::default();

    // Create a Ranging regime with LOW volatility (< 1.5)
    let ranging_regime = MarketRegime::new(
        MarketRegimeType::Ranging,
        dec!(0.8),  // High confidence
        dec!(1.0),  // Low volatility -> MeanReversion
        dec!(10.0), // Low trend strength
    );

    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should switch to ZScoreMR for Ranging regime
    assert_eq!(
        new_mode,
        StrategyMode::ZScoreMR,
        "Should select ZScoreMR strategy for Ranging regime"
    );
}

#[test]
fn test_strategy_selector_trending_up_to_trend_riding() {
    let config = AnalystConfig::default();

    // Create a TrendingUp regime
    let trending_regime = MarketRegime::new(
        MarketRegimeType::TrendingUp,
        dec!(0.9),  // High confidence
        dec!(1.5),  // Moderate volatility
        dec!(35.0), // High trend strength
    );

    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&trending_regime, &config, current_mode);

    // Should switch to StatMomentum for trending markets
    assert_eq!(
        new_mode,
        StrategyMode::StatMomentum,
        "Should select StatMomentum strategy for TrendingUp regime"
    );
}

#[test]
fn test_strategy_selector_trending_down_to_trend_riding() {
    let config = AnalystConfig::default();

    let trending_regime = MarketRegime::new(
        MarketRegimeType::TrendingDown,
        dec!(0.85),
        dec!(2.0),
        dec!(40.0),
    );

    let current_mode = StrategyMode::Standard;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&trending_regime, &config, current_mode);

    assert_eq!(
        new_mode,
        StrategyMode::StatMomentum,
        "Should select StatMomentum strategy for TrendingDown regime"
    );
}

#[test]
fn test_strategy_selector_volatile_to_momentum() {
    let config = AnalystConfig::default();

    let volatile_regime = MarketRegime::new(
        MarketRegimeType::Volatile,
        dec!(0.7),
        dec!(5.0), // High volatility
        dec!(15.0),
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

    // Low volatility ranging -> ZScoreMR
    let ranging_regime =
        MarketRegime::new(MarketRegimeType::Ranging, dec!(0.8), dec!(1.0), dec!(10.0));

    // Already using ZScoreMR
    let current_mode = StrategyMode::ZScoreMR;

    let (new_mode, _strategy) =
        StrategySelector::select_strategy(&ranging_regime, &config, current_mode);

    // Should stay with ZScoreMR
    assert_eq!(
        new_mode,
        StrategyMode::ZScoreMR,
        "Should keep ZScoreMR when already appropriate for Ranging"
    );
}
