mod advanced;
mod breakout;
mod dual_sma;
mod dynamic;
mod ensemble;
mod mean_reversion;
mod momentum;
mod smc;
pub mod strategy_selector;
mod traits;
pub mod trend_riding;
mod vwap;

pub use advanced::{AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy};
pub use breakout::BreakoutStrategy;
pub use dual_sma::DualSMAStrategy;
pub use dynamic::{DynamicRegimeConfig, DynamicRegimeStrategy};
pub use ensemble::EnsembleStrategy;
pub use mean_reversion::MeanReversionStrategy;
pub use momentum::MomentumDivergenceStrategy;
pub use smc::SMCStrategy;
pub use traits::{AnalysisContext, Signal, TradingStrategy};
pub use trend_riding::TrendRidingStrategy;
pub use vwap::VWAPStrategy;

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
            StrategyMode::Dynamic => {
                Arc::new(DynamicRegimeStrategy::with_config(DynamicRegimeConfig {
                    fast_period: config.fast_sma_period,
                    slow_period: config.slow_sma_period,
                    sma_threshold: config.sma_threshold,
                    trend_sma_period: config.trend_sma_period,
                    rsi_threshold: config.rsi_threshold,
                    trend_divergence_threshold: config.trend_divergence_threshold,
                    // Risk-appetite adaptive parameters now properly passed!
                    signal_confirmation_bars: config.signal_confirmation_bars,
                    macd_requires_rising: config.macd_requires_rising,
                    trend_tolerance_pct: config.trend_tolerance_pct,
                    macd_min_threshold: config.macd_min_threshold,
                    adx_threshold: config.adx_threshold,
                }))
            }
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
            StrategyMode::SMC => Arc::new(SMCStrategy::new(
                config.smc_ob_lookback,
                config.smc_min_fvg_size_pct,
            )),
            StrategyMode::VWAP => Arc::new(VWAPStrategy::default()),
            StrategyMode::Breakout => Arc::new(BreakoutStrategy::default()),
            StrategyMode::Momentum => Arc::new(MomentumDivergenceStrategy::default()),
            StrategyMode::Ensemble => Arc::new(EnsembleStrategy::default_ensemble()),
        }
    }
}
