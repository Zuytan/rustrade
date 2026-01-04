mod advanced;
mod dual_sma;
mod dynamic;
mod mean_reversion;
pub mod strategy_selector;
mod traits;
pub mod trend_riding;

pub use advanced::{AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy};
pub use dual_sma::DualSMAStrategy;
pub use dynamic::DynamicRegimeStrategy;
pub use mean_reversion::MeanReversionStrategy;
pub use traits::{AnalysisContext, Signal, TradingStrategy};
pub use trend_riding::TrendRidingStrategy;

use crate::application::agents::analyst::AnalystConfig;
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
                AdvancedTripleFilterConfig {
                    fast_period: config.fast_sma_period,
                    slow_period: config.slow_sma_period,
                    sma_threshold: config.sma_threshold,
                    trend_sma_period: config.trend_sma_period,
                    rsi_threshold: config.rsi_threshold,
                    signal_confirmation_bars: config.signal_confirmation_bars,
                    macd_requires_rising: config.macd_requires_rising,
                    trend_tolerance_pct: config.trend_tolerance_pct,
                    macd_min_threshold: config.macd_min_threshold,
                    adx_threshold: config.adx_threshold,
                },
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
            StrategyMode::RegimeAdaptive => Arc::new(TrendRidingStrategy::new(
                config.fast_sma_period,
                config.slow_sma_period,
                config.sma_threshold,
                config.trend_riding_exit_buffer_pct,
            )),
        }
    }
}
