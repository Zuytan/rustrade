//! Market Regime Handler
//!
//! Handles market regime detection and dynamic risk scaling based on market conditions.
//! Extracted from [`Analyst`] to reduce module complexity.

use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::repositories::CandleRepository;
use crate::domain::risk::risk_appetite::RiskAppetite;
use crate::domain::trading::symbol_context::SymbolContext;
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
    if let (Some(hurst), Some(vol)) = (
        context.last_features.hurst_exponent,
        context.last_features.realized_volatility,
    ) {
        #[allow(clippy::collapsible_if)]
        if let Ok(regime) = context.regime_detector.detect_from_features(
            Some(hurst),
            Some(vol),
            context.last_features.skewness,
        ) {
            return regime;
        }
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

/// Applies dynamic risk scaling based on market regime.
///
/// Automatically lowers risk in volatile or bearish regimes to protect capital.
///
/// # Arguments
/// * `context` - Symbol context to modify
/// * `regime` - Current market regime
/// * `symbol` - Symbol name for logging
///
/// # Risk Modifiers
/// - **Volatile**: -3 risk score
/// - **TrendingDown**: -2 risk score
/// - **Other regimes**: No modification
pub fn apply_dynamic_risk_scaling(
    context: &mut SymbolContext,
    regime: &MarketRegime,
    symbol: &str,
) {
    if let Some(base_score) = context.config.risk_appetite_score {
        let modifier = match regime.regime_type {
            MarketRegimeType::Volatile => -3,
            MarketRegimeType::TrendingDown => -2,
            _ => 0,
        };

        if modifier != 0 {
            let new_score = (base_score as i8 + modifier).clamp(1, 9) as u8;
            if let Ok(new_appetite) = RiskAppetite::new(new_score) {
                context.config.apply_risk_appetite(&new_appetite);
                // Also update the stored score
                context.config.risk_appetite_score = Some(new_score);

                // Re-initialize the active strategy to pick up new risk parameters
                // (e.g. RSI thresholds, stop multipliers, etc.)
                context.strategy = crate::application::strategies::StrategyFactory::create(
                    context.active_strategy_mode,
                    &context.config,
                );

                info!(
                    "RegimeHandler [{}]: Dynamic Risk Scaling active. Score {} -> {} ({:?}). Strategy re-initialized.",
                    symbol, base_score, new_score, regime.regime_type
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
    symbol: &str,
) -> bool {
    use crate::application::strategies::strategy_selector::StrategySelector;
    use crate::domain::market::strategy_config::StrategyMode;

    if config.strategy_mode != StrategyMode::RegimeAdaptive {
        return false;
    }

    let (new_mode, new_strategy) =
        StrategySelector::select_strategy(regime, config, context.active_strategy_mode);

    if new_mode != context.active_strategy_mode {
        info!(
            "RegimeHandler: Adaptive Switch for {} -> {:?} (Regime: {:?})",
            symbol, new_mode, regime.regime_type
        );
        context.strategy = new_strategy;
        context.active_strategy_mode = new_mode;
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agents::analyst_config::AnalystConfig;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::DualSMAStrategy;
    use crate::domain::market::market_regime::MarketRegimeType;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn create_test_context() -> SymbolContext {
        let config = AnalystConfig::default();
        let strategy = Arc::new(DualSMAStrategy::new(20, 60, Decimal::ZERO));
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        SymbolContext::new(config, strategy, win_rate_provider, vec![])
    }

    #[test]
    fn test_dynamic_risk_scaling_volatile() {
        let mut context = create_test_context();
        context.config.risk_appetite_score = Some(7);

        let regime =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.8), dec!(3.0), dec!(15.0));

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        // Score should be reduced: 7 - 3 = 4
        assert_eq!(context.config.risk_appetite_score, Some(4));
    }

    #[test]
    fn test_dynamic_risk_scaling_trending_down() {
        let mut context = create_test_context();
        context.config.risk_appetite_score = Some(5);

        let regime = MarketRegime::new(
            MarketRegimeType::TrendingDown,
            dec!(0.7),
            dec!(1.5),
            dec!(25.0),
        );

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        // Score should be reduced: 5 - 2 = 3
        assert_eq!(context.config.risk_appetite_score, Some(3));
    }

    #[test]
    fn test_dynamic_risk_scaling_no_change_on_trending_up() {
        let mut context = create_test_context();
        context.config.risk_appetite_score = Some(5);

        let regime = MarketRegime::new(
            MarketRegimeType::TrendingUp,
            dec!(0.8),
            dec!(1.0),
            dec!(30.0),
        );

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        // Score should not change for bullish regime
        assert_eq!(context.config.risk_appetite_score, Some(5));
    }

    #[test]
    fn test_dynamic_risk_scaling_clamps_to_min() {
        let mut context = create_test_context();
        context.config.risk_appetite_score = Some(2);

        let regime =
            MarketRegime::new(MarketRegimeType::Volatile, dec!(0.9), dec!(4.0), dec!(10.0));

        apply_dynamic_risk_scaling(&mut context, &regime, "TEST");

        // Score should be clamped: max(2 - 3, 1) = 1
        assert_eq!(context.config.risk_appetite_score, Some(1));
    }
}
