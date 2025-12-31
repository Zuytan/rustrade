
use crate::application::strategies::{TradingStrategy, StrategyFactory}; // Assuming Factory exists or we use direct instantiation
use crate::application::agents::analyst::AnalystConfig;
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::market::strategy_config::StrategyMode;
use std::sync::Arc;
use tracing::info;

pub struct StrategySelector;

impl StrategySelector {
    /// Selects the best strategy for the given market regime.
    /// 
    /// Default Logic:
    /// - TrendingUp/Down -> TrendRiding (or Advanced)
    /// - Ranging -> MeanReversion
    /// - Volatile -> MeanReversion (tuned) or Protective
    /// - Unknown -> Standard/Defensive
    pub fn select_strategy(
        regime: &MarketRegime,
        config: &AnalystConfig,
        current_mode: StrategyMode,
    ) -> (StrategyMode, Arc<dyn TradingStrategy>) {
        
        let proposed_mode = match regime.regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                // In strong trends, prefer TrendRiding
                 StrategyMode::TrendRiding
            }
            MarketRegimeType::Ranging => {
                // In choppy sideways markets, Mean Reversion works best
                StrategyMode::MeanReversion
            }
            MarketRegimeType::Volatile => {
                // Volatile is tricky. Mean Reversion with wide bands? Or Standard?
                // Let's stick to Mean Reversion but maybe the config handles the width.
                StrategyMode::MeanReversion
            }
            MarketRegimeType::Unknown => {
                // Fallback
                StrategyMode::Standard
            }
        };

        // If no change, return what we have (optimization: don't re-instantiate if not needed, 
        // though strictly we return a new instance here for simplicity unless we pass the old one).
        // Actually, the caller holds the instance. The caller should check if mode changed.
        // But here we are asked to return the strategy object.
        
        if proposed_mode == current_mode {
             // We can't return the old instance easily without passing it in.
             // But usually creating these strategies is cheap.
             // Let's Log if we are "keeping" on paper, but we re-create for now.
             // Ideally the caller checks the Mode enum before calling this factory if they want to avoid churn.
        } else {
             info!("StrategySelector: Switching strategy from {} to {} based on Regime {:?}", current_mode, proposed_mode, regime.regime_type);
        }

        let strategy = StrategyFactory::create(proposed_mode, config);
        (proposed_mode, strategy)
    }
}
