use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::market::strategy_config::StrategyMode;

pub struct StrategySelector;

impl StrategySelector {
    /// Selects the best strategy mode based on the current market regime.
    /// If an override mode is provided (not RegimeAdaptive), it will be used instead.
    pub fn select_best(
        regime: MarketRegime,
        _symbol: &str,
        _config: &AnalystConfig,
        current_mode: StrategyMode,
    ) -> StrategyMode {
        // If user manually selected a specific strategy, respect it
        if current_mode != StrategyMode::RegimeAdaptive {
            return current_mode;
        }

        // Otherwise, adapt based on regime
        match regime.regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                StrategyMode::RegimeAdaptive
            }
            MarketRegimeType::Volatile => StrategyMode::SMC,
            MarketRegimeType::Ranging => StrategyMode::ZScoreMR,
            MarketRegimeType::Unknown => StrategyMode::RegimeAdaptive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agents::analyst_config::AnalystConfig;
    use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
    use rust_decimal_macros::dec;

    fn make_regime(t: MarketRegimeType) -> MarketRegime {
        MarketRegime::new(t, dec!(0.8), dec!(0.5), dec!(0.5))
    }

    #[test]
    fn test_select_strategy_for_regimes() {
        let config = AnalystConfig::default();
        let name = "BTCUSDT";

        // Trending -> RegimeAdaptive
        let mode = StrategySelector::select_best(
            make_regime(MarketRegimeType::TrendingUp),
            name,
            &config,
            StrategyMode::RegimeAdaptive,
        );
        assert_eq!(mode, StrategyMode::RegimeAdaptive);

        // Volatile -> SMC
        let mode = StrategySelector::select_best(
            make_regime(MarketRegimeType::Volatile),
            name,
            &config,
            StrategyMode::RegimeAdaptive,
        );
        assert_eq!(mode, StrategyMode::SMC);

        // Sideways (Ranging) -> ZScoreMR
        let mode = StrategySelector::select_best(
            make_regime(MarketRegimeType::Ranging),
            name,
            &config,
            StrategyMode::RegimeAdaptive,
        );
        assert_eq!(mode, StrategyMode::ZScoreMR);

        // Force manual override
        let mode = StrategySelector::select_best(
            make_regime(MarketRegimeType::TrendingUp),
            name,
            &config,
            StrategyMode::SMC,
        );
        assert_eq!(mode, StrategyMode::SMC);
    }
}
