//! Market Regime Handler
//!
//! Handles market regime detection and dynamic risk scaling based on market conditions.
//! Extracted from [`Analyst`] to reduce module complexity.

use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::trading::symbol_context::SymbolContext;
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::repositories::CandleRepository;
use crate::domain::risk::risk_appetite::RiskAppetite;
use std::sync::Arc;
use tracing::info;

/// Detects market regime for a symbol using historical candle data.
///
/// # Arguments
/// * `repo` - Optional candle repository for fetching historical data
/// * `symbol` - The trading symbol
/// * `candle_timestamp` - Current candle timestamp
/// * `context` - Symbol context with regime detector
///
/// # Returns
/// The detected market regime, or `MarketRegime::unknown()` if detection fails.
pub async fn detect_market_regime(
    repo: &Option<Arc<dyn CandleRepository>>,
    symbol: &str,
    candle_timestamp: i64,
    context: &SymbolContext,
) -> MarketRegime {
    // 1. Try Fast Feature-based Detection (Phase 4 Enhanced)
    // If we have calculated features (Hurst, Volatility) for the current candle, use them.
    // This is O(1) compared to O(N) fetching and processing historical candles.
    if let Some(regime) = context
        .last_features
        .hurst_exponent
        .zip(context.last_features.realized_volatility)
        .and_then(|(hurst, vol)| {
            context
                .regime_detector
                .detect_from_features(Some(hurst), Some(vol), context.last_features.skewness)
                .ok()
        })
    {
        return regime;
    }

    // 2. Fallback to Historical Candle Analysis
    // Necessary during warmup or if advanced features are not yet ready (need ~50 bars)
    if let Some(repo) = repo {
        let end_ts = candle_timestamp;
        let start_ts = end_ts - (30 * 24 * 60 * 60); // 30 days lookback

        if let Ok(candles) = repo.get_range(symbol, start_ts, end_ts).await {
            return context
                .regime_detector
                .detect(&candles)
                .unwrap_or(MarketRegime::unknown());
        }
    }
    MarketRegime::unknown()
}

/// Number of consecutive "normal" regime bars required before restoring base risk (hysteresis).
const RISK_RESTORE_HYSTERESIS_BARS: u32 = 5;

/// Applies dynamic risk scaling based on market regime, with hysteresis.
///
/// - In **Volatile** or **TrendingDown**: reduces risk immediately; stores base score and starts
///   a countdown. Restore only after `RISK_RESTORE_HYSTERESIS_BARS` consecutive bars in a
///   non-hostile regime, to avoid whipsaw.
///
/// # Risk Modifiers
/// - **Volatile**: -3 risk score
/// - **TrendingDown**: -2 risk score
/// - **Other regimes**: no reduction; if risk was reduced, decrement restore countdown and
///   restore base score when countdown reaches 0.
pub fn apply_dynamic_risk_scaling(
    context: &mut SymbolContext,
    regime: &MarketRegime,
    symbol: &str,
) {
    let modifier = match regime.regime_type {
        MarketRegimeType::Volatile => -3,
        MarketRegimeType::TrendingDown => -2,
        _ => 0,
    };

    if modifier != 0 {
        // Hostile regime: reduce risk (from base score), set or keep base, reset countdown
        let current = context.config.strategy.risk_appetite_score;
        if let Some(current_score) = current {
            let base = context.risk_base_score.unwrap_or(current_score);
            if context.risk_base_score.is_none() {
                context.risk_base_score = Some(base);
            }
            let new_score = (base as i8 + modifier).clamp(1, 9) as u8;
            if Some(new_score) != context.config.strategy.risk_appetite_score {
                if let Ok(new_appetite) = RiskAppetite::new(new_score) {
                    context.config.apply_risk_appetite(&new_appetite);
                    context.config.strategy.risk_appetite_score = Some(new_score);
                    context.risk_restore_bars_remaining = Some(RISK_RESTORE_HYSTERESIS_BARS);
                    context.strategy = crate::application::strategies::StrategyFactory::create(
                        context.active_strategy_mode,
                        &context.config,
                    );
                    info!(
                        "RegimeHandler [{}]: Dynamic Risk Scaling. Score {} -> {} ({:?}). Restore after {} normal bars.",
                        symbol, base, new_score, regime.regime_type, RISK_RESTORE_HYSTERESIS_BARS
                    );
                }
            } else {
                // Prolong countdown even if score didn't change (still in hostile regime)
                context.risk_restore_bars_remaining = Some(RISK_RESTORE_HYSTERESIS_BARS);
            }
        }
        return;
    }

    // Normal regime: hysteresis countdown toward restore
    if let Some(remaining) = context.risk_restore_bars_remaining.as_mut() {
        if *remaining > 0 {
            *remaining -= 1;
        }
        if *remaining == 0
            && let Some(base) = context.risk_base_score.take()
        {
            context.risk_restore_bars_remaining = None;
            if let Ok(restore_appetite) = RiskAppetite::new(base) {
                context.config.apply_risk_appetite(&restore_appetite);
                context.config.strategy.risk_appetite_score = Some(base);
                context.strategy = crate::application::strategies::StrategyFactory::create(
                    context.active_strategy_mode,
                    &context.config,
                );
                info!(
                    "RegimeHandler [{}]: Risk restored to base score {} after hysteresis.",
                    symbol, base
                );
            }
        }
    }
}

/// Applies adaptive strategy switching based on market regime.
///
/// When in `RegimeAdaptive` mode, automatically switches strategy based on
/// the current market conditions (trending, ranging, volatile).
///
/// # Arguments
/// * `context` - Symbol context to modify
/// * `regime` - Current market regime
/// * `config` - Analyst config for strategy selection
/// * `symbol` - Symbol name for logging
///
/// # Returns
/// `true` if strategy was switched, `false` otherwise.
pub fn apply_adaptive_strategy_switching(
    context: &mut SymbolContext,
    regime: &MarketRegime,
    config: &AnalystConfig,
    _symbol: &str,
) -> bool {
    use crate::application::strategies::strategy_selector::StrategySelector;
    use crate::domain::market::strategy_config::StrategyMode;

    if config.strategy.strategy_mode != StrategyMode::RegimeAdaptive {
        return false;
    }

    // Update strategy mode based on regime
    let symbol = context.active_symbol();
    let new_mode = StrategySelector::select_best(
        regime.clone(),
        &symbol,
        config,
        context.active_strategy_mode,
    );

    if new_mode != context.active_strategy_mode {
        info!(
            "Regime change detected for {}: {:?} -> {:?}",
            symbol, context.active_strategy_mode, new_mode
        );
        context.active_strategy_mode = new_mode;
        // Re-create the strategy instance using the factor
        use crate::application::strategies::StrategyFactory;
        context.strategy = StrategyFactory::create(new_mode, config);
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agents::analyst_config::AnalystConfig;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::SMCStrategy as DualSMAStrategy;
    use crate::domain::market::market_regime::MarketRegimeType;
    use rust_decimal_macros::dec;

    fn create_test_context_with_score(score: Option<u8>) -> SymbolContext {
        let mut config = AnalystConfig::default();
        config.strategy.risk_appetite_score = score;
        let strategy = Arc::new(DualSMAStrategy::new(20, dec!(0.001), dec!(1.0)));
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        SymbolContext::new(config, strategy, win_rate_provider, vec![])
    }

    #[test]
    fn test_dynamic_risk_scaling_volatile() {
        let mut context = create_test_context_with_score(Some(7));

        let regime =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.8), dec!(3.0), dec!(15.0));

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        assert_eq!(context.config.strategy.risk_appetite_score, Some(4));
        assert_eq!(context.risk_base_score, Some(7));
        assert_eq!(context.risk_restore_bars_remaining, Some(5));
    }

    #[test]
    fn test_dynamic_risk_scaling_trending_down() {
        let mut context = create_test_context_with_score(Some(5));

        let regime = MarketRegime::new(
            MarketRegimeType::TrendingDown,
            dec!(0.7),
            dec!(1.5),
            dec!(25.0),
        );

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        // Score should be reduced: 5 - 2 = 3
        assert_eq!(context.config.strategy.risk_appetite_score, Some(3));
    }

    #[test]
    fn test_dynamic_risk_scaling_no_change_on_trending_up() {
        let mut context = create_test_context_with_score(Some(5));

        let regime = MarketRegime::new(
            MarketRegimeType::TrendingUp,
            dec!(0.8),
            dec!(1.0),
            dec!(30.0),
        );

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        // Score should not change for bullish regime
        assert_eq!(context.config.strategy.risk_appetite_score, Some(5));
    }

    #[test]
    fn test_dynamic_risk_scaling_clamps_to_min() {
        let mut context = create_test_context_with_score(Some(2));

        let regime =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.9), dec!(4.0), dec!(10.0));

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        assert_eq!(context.config.strategy.risk_appetite_score, Some(1));
    }

    #[test]
    fn test_hysteresis_restore_after_normal_bars() {
        let mut context = create_test_context_with_score(Some(7));

        let volatile =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.8), dec!(3.0), dec!(15.0));
        apply_dynamic_risk_scaling(&mut context, &volatile, "TEST");
        assert_eq!(context.config.strategy.risk_appetite_score, Some(4));

        let normal = MarketRegime::new(
            MarketRegimeType::TrendingUp,
            dec!(0.8),
            dec!(1.0),
            dec!(30.0),
        );
        for _ in 0..RISK_RESTORE_HYSTERESIS_BARS {
            apply_dynamic_risk_scaling(&mut context, &normal, "TEST");
        }
        assert_eq!(context.config.strategy.risk_appetite_score, Some(7));
        assert_eq!(context.risk_base_score, None);
        assert_eq!(context.risk_restore_bars_remaining, None);
    }

    #[test]
    fn test_hysteresis_reset_on_volatile_again() {
        let mut context = create_test_context_with_score(Some(7));

        let volatile =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.8), dec!(3.0), dec!(15.0));
        apply_dynamic_risk_scaling(&mut context, &volatile, "TEST");
        assert_eq!(context.risk_restore_bars_remaining, Some(5));

        let normal = MarketRegime::new(MarketRegimeType::Ranging, dec!(0.5), dec!(1.0), dec!(20.0));
        apply_dynamic_risk_scaling(&mut context, &normal, "TEST");
        apply_dynamic_risk_scaling(&mut context, &normal, "TEST");
        assert_eq!(context.risk_restore_bars_remaining, Some(3));

        apply_dynamic_risk_scaling(&mut context, &volatile, "TEST");
        assert_eq!(context.config.strategy.risk_appetite_score, Some(4));
        assert_eq!(context.risk_restore_bars_remaining, Some(5));
    }
    #[test]
    fn test_regime_handler_no_recreation_when_score_unchanged() {
        let mut context = create_test_context_with_score(Some(4));
        let strategy_ptr = Arc::clone(&context.strategy);

        // Volatile regime reduces score by 3. If base is 7, it becomes 4.
        // If current is already 4, it should calculate 4, see it's unchanged, and NOT recreate strategy.
        context.risk_base_score = Some(7);
        let volatile =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.8), dec!(3.0), dec!(15.0));

        apply_dynamic_risk_scaling(&mut context, &volatile, "TEST");

        assert_eq!(context.config.strategy.risk_appetite_score, Some(4));
        assert!(
            Arc::ptr_eq(&context.strategy, &strategy_ptr),
            "Strategy should not be recreated if score is unchanged"
        );
    }

    #[test]
    fn test_regime_handler_prolongs_countdown_when_score_same() {
        let mut context = create_test_context_with_score(Some(4));
        context.risk_base_score = Some(7);
        context.risk_restore_bars_remaining = Some(2); // Countdown was at 2

        let volatile =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.8), dec!(3.0), dec!(15.0));

        // This is a hostile regime, so score will stay 4, but countdown should reset to 5
        apply_dynamic_risk_scaling(&mut context, &volatile, "TEST");

        assert_eq!(context.risk_restore_bars_remaining, Some(5));
    }
}
