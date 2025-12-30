use crate::application::agents::analyst::AnalystConfig;
use crate::application::strategies::{
    AdvancedTripleFilterStrategy, DualSMAStrategy, DynamicRegimeStrategy, MeanReversionStrategy,
    TradingStrategy, TrendRidingStrategy,
};
use crate::domain::market::strategy_config::StrategyMode;
use std::sync::Arc;

pub struct StrategyFactory;

impl StrategyFactory {
    pub fn create(mode: StrategyMode, config: &AnalystConfig) -> Arc<dyn TradingStrategy> {
        match mode {
            StrategyMode::Standard => Arc::new(DualSMAStrategy::new(
                config.fast_sma_period,
                config.slow_sma_period,
                config.sma_threshold,
            )),
            StrategyMode::Advanced => Arc::new(AdvancedTripleFilterStrategy::new(
                config.fast_sma_period,
                config.slow_sma_period,
                config.sma_threshold,
                config.trend_sma_period,
                config.rsi_threshold,
                config.signal_confirmation_bars,
            )),
            StrategyMode::Dynamic => Arc::new(DynamicRegimeStrategy::new(
                config.fast_sma_period,
                config.slow_sma_period,
                config.sma_threshold,
                config.trend_sma_period,
                config.rsi_threshold,
                config.trend_divergence_threshold,
            )),
            StrategyMode::TrendRiding => Arc::new(TrendRidingStrategy::new(
                config.fast_sma_period,
                config.slow_sma_period,
                config.sma_threshold,
                config.trend_riding_exit_buffer_pct,
            )),
            StrategyMode::MeanReversion => Arc::new(MeanReversionStrategy::new(
                config.mean_reversion_bb_period,
                config.mean_reversion_rsi_exit,
            )),
        }
    }
}
