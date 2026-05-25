use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::strategies::{
    EnsembleStrategy, OrderFlowStrategy, SMCStrategy, StatisticalMomentumStrategy, TradingStrategy,
    ZScoreMeanReversionStrategy, snn_surrogate_strategy::SnnSurrogateStrategy,
};
use crate::domain::market::strategy_config::StrategyMode;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

pub struct StrategyFactory;

impl StrategyFactory {
    pub fn create(mode: StrategyMode, config: &AnalystConfig) -> Arc<dyn TradingStrategy> {
        match mode {
            StrategyMode::RegimeAdaptive => {
                // RegimeAdaptive uses Ensemble logic or specialized routing.
                // For now, routing to SMC as a high-performance modern default for adaptive regimes,
                // or preferably the actual EnsembleStrategy.
                Arc::new(EnsembleStrategy::modern_ensemble(config))
            }
            StrategyMode::SMC => Arc::new(SMCStrategy::new(
                config.strategy.smc_ob_lookback,
                config.strategy.smc_min_fvg_size_pct,
                config.strategy.smc_volume_multiplier,
            )),
            StrategyMode::Ensemble => Arc::new(EnsembleStrategy::modern_ensemble(config)),
            // Modern statistical/microstructure strategies (params from config)
            StrategyMode::ZScoreMR => Arc::new(ZScoreMeanReversionStrategy::new(
                config.strategy.zscore_lookback,
                config.strategy.zscore_entry_threshold,
                config.strategy.zscore_exit_threshold,
            )),
            StrategyMode::StatMomentum => Arc::new(StatisticalMomentumStrategy::new(
                config.strategy.stat_momentum_lookback,
                config.strategy.stat_momentum_threshold,
                config.strategy.stat_momentum_trend_confirmation,
            )),
            StrategyMode::OrderFlow => Arc::new(OrderFlowStrategy::new(
                config.strategy.orderflow_ofi_threshold,
                config.strategy.orderflow_stacked_count,
                config.strategy.orderflow_volume_profile_lookback,
            )),
            StrategyMode::ML => {
                tracing::warn!(
                    "ML strategy mode is no longer supported. Falling back to Ensemble."
                );
                Arc::new(EnsembleStrategy::modern_ensemble(config))
            }
            StrategyMode::SnnSurrogate => {
                match SnnSurrogateStrategy::new(
                    &config.strategy.snn_surrogate_model_path,
                    config
                        .strategy
                        .snn_activation_threshold
                        .to_f64()
                        .unwrap_or(0.3),
                    config.strategy.snn_surrogate_window_size,
                    config
                        .strategy
                        .snn_encoder_threshold
                        .to_f64()
                        .unwrap_or(0.5),
                ) {
                    Ok(strategy) => Arc::new(strategy),
                    Err(e) => {
                        tracing::error!(
                            "Failed to load SNN Surrogate strategy: {}. Falling back to Ensemble.",
                            e
                        );
                        // Fallback to Ensemble if surrogate fails
                        Arc::new(EnsembleStrategy::modern_ensemble(config))
                    }
                }
            }
        }
    }
}
