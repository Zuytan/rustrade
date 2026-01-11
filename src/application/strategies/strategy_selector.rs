use crate::application::agents::analyst::AnalystConfig;
use crate::application::strategies::{StrategyFactory, TradingStrategy};
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::market::strategy_config::StrategyMode;
use std::sync::Arc;
use tracing::info;

pub struct StrategySelector;

impl StrategySelector {
    /// Selects the best strategy for the given market regime.
    ///
    /// Enhanced Logic (v0.60.0):
    /// - **TrendingUp/Down** → TrendRiding (strong momentum capture)
    /// - **Ranging** → VWAP (institutional mean-reversion around VWAP)  
    /// - **Volatile** → Momentum (divergence detection for reversals)
    /// - **Unknown** → Standard (safe fallback)
    ///
    /// Optional: Use Breakout when transitioning FROM Ranging TO Trending
    pub fn select_strategy(
        regime: &MarketRegime,
        config: &AnalystConfig,
        current_mode: StrategyMode,
    ) -> (StrategyMode, Arc<dyn TradingStrategy>) {
        let proposed_mode = Self::select_mode_for_regime(regime, current_mode);

        if proposed_mode != current_mode {
            info!(
                "StrategySelector: Switching strategy from {} to {} based on Regime {:?} (strength: {:.1}%)",
                current_mode,
                proposed_mode,
                regime.regime_type,
                regime.confidence * 100.0
            );
        }

        let strategy = StrategyFactory::create(proposed_mode, config);
        (proposed_mode, strategy)
    }

    /// Core logic for mapping regime to strategy mode
    ///
    /// Enhanced with hysteresis: requires high confidence (>= 0.6) to switch strategies,
    /// preventing whipsaw from rapid regime changes.
    fn select_mode_for_regime(regime: &MarketRegime, current_mode: StrategyMode) -> StrategyMode {
        // Hysteresis: Only switch if confidence is high enough
        // This prevents rapid switching (whipsawing) between strategies
        const MIN_CONFIDENCE_TO_SWITCH: f64 = 0.6;

        if regime.confidence < MIN_CONFIDENCE_TO_SWITCH && current_mode != StrategyMode::Standard {
            // Low confidence in new regime - stick with current strategy
            return current_mode;
        }

        match regime.regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                // Strong trends → Trend following
                // But if coming from Ranging, we might be seeing a Breakout
                if current_mode == StrategyMode::VWAP || current_mode == StrategyMode::MeanReversion
                {
                    // Potential breakout from consolidation!
                    // Use Breakout for the first leg, then switch to TrendRiding
                    if regime.confidence < 0.7 {
                        StrategyMode::Breakout
                    } else {
                        StrategyMode::TrendRiding
                    }
                } else {
                    StrategyMode::TrendRiding
                }
            }
            MarketRegimeType::Ranging => {
                // Sideways/consolidation → Mean Reversion strategies
                // Use MeanReversion if volatility is low, VWAP otherwise
                if regime.volatility_score < 1.5 {
                    // Low volatility ranging - classic mean reversion
                    StrategyMode::MeanReversion
                } else {
                    // Higher volatility ranging - use VWAP for tighter control
                    StrategyMode::VWAP
                }
            }
            MarketRegimeType::Volatile => {
                // High volatility → Momentum divergence detection
                // Divergences often signal reversals in volatile conditions
                StrategyMode::Momentum
            }
            MarketRegimeType::Unknown => {
                // No clear regime → Safe fallback
                StrategyMode::Standard
            }
        }
    }

    /// Alternative: Select Ensemble mode for maximum robustness
    /// (combines multiple strategies with voting)
    #[allow(dead_code)]
    pub fn select_ensemble_strategy(
        config: &AnalystConfig,
    ) -> (StrategyMode, Arc<dyn TradingStrategy>) {
        info!("StrategySelector: Using Ensemble mode (multi-strategy voting)");
        let strategy = StrategyFactory::create(StrategyMode::Ensemble, config);
        (StrategyMode::Ensemble, strategy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> AnalystConfig {
        AnalystConfig::default()
    }

    fn make_regime(regime_type: MarketRegimeType, confidence: f64) -> MarketRegime {
        MarketRegime {
            regime_type,
            confidence,
            volatility_score: 1.5,
            trend_strength: 30.0,
        }
    }

    #[test]
    fn test_trending_uses_trend_riding() {
        let config = default_config();
        let regime = make_regime(MarketRegimeType::TrendingUp, 0.8);

        let (mode, _) = StrategySelector::select_strategy(&regime, &config, StrategyMode::Standard);
        assert_eq!(mode, StrategyMode::TrendRiding);
    }

    #[test]
    fn test_ranging_uses_vwap() {
        let config = default_config();
        let regime = make_regime(MarketRegimeType::Ranging, 0.7);

        let (mode, _) = StrategySelector::select_strategy(&regime, &config, StrategyMode::Standard);
        assert_eq!(mode, StrategyMode::VWAP);
    }

    #[test]
    fn test_volatile_uses_momentum() {
        let config = default_config();
        let regime = make_regime(MarketRegimeType::Volatile, 0.75);

        let (mode, _) = StrategySelector::select_strategy(&regime, &config, StrategyMode::Standard);
        assert_eq!(mode, StrategyMode::Momentum);
    }

    #[test]
    fn test_breakout_on_range_to_trend_transition() {
        let config = default_config();
        // Was in VWAP (Ranging), now detecting early TrendingUp
        let regime = make_regime(MarketRegimeType::TrendingUp, 0.6); // Low confidence = early trend

        let (mode, _) = StrategySelector::select_strategy(&regime, &config, StrategyMode::VWAP);
        assert_eq!(
            mode,
            StrategyMode::Breakout,
            "Should use Breakout when transitioning from Range to Trend"
        );
    }

    #[test]
    fn test_unknown_uses_standard() {
        let config = default_config();
        let regime = make_regime(MarketRegimeType::Unknown, 0.3);

        let (mode, _) = StrategySelector::select_strategy(&regime, &config, StrategyMode::Standard);
        assert_eq!(mode, StrategyMode::Standard);
    }
}
