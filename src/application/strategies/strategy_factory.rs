use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::strategies::{
    AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy, BreakoutStrategy, DualSMAStrategy,
    DynamicRegimeConfig, DynamicRegimeStrategy, EnsembleStrategy, MeanReversionStrategy,
    MomentumDivergenceStrategy, OrderFlowStrategy, SMCStrategy, StatisticalMomentumStrategy,
    TradingStrategy, TrendRidingStrategy, VWAPStrategy, ZScoreMeanReversionStrategy,
};
use crate::domain::market::strategy_config::StrategyMode;
use std::sync::Arc;

pub struct StrategyFactory;

impl StrategyFactory {
    pub fn create(mode: StrategyMode, config: &AnalystConfig) -> Arc<dyn TradingStrategy> {
        match mode {
            StrategyMode::Standard => Arc::new(DualSMAStrategy::new(
                config.strategy.fast_sma_period,
                config.strategy.slow_sma_period,
                config.strategy.sma_threshold,
            )),
            StrategyMode::Advanced => Arc::new(AdvancedTripleFilterStrategy::new(
                AdvancedTripleFilterConfig {
                    fast_period: config.strategy.fast_sma_period,
                    slow_period: config.strategy.slow_sma_period,
                    sma_threshold: config.strategy.sma_threshold,
                    trend_sma_period: config.strategy.trend_sma_period,
                    rsi_threshold: config.strategy.rsi_threshold,
                    signal_confirmation_bars: config.strategy.signal_confirmation_bars,
                    macd_requires_rising: config.strategy.macd_requires_rising,
                    trend_tolerance_pct: config.strategy.trend_tolerance_pct,
                    macd_min_threshold: config.strategy.macd_min_threshold,
                    adx_threshold: config.strategy.adx_threshold,
                },
            )),
            StrategyMode::Dynamic => {
                Arc::new(DynamicRegimeStrategy::with_config(DynamicRegimeConfig {
                    fast_period: config.strategy.fast_sma_period,
                    slow_period: config.strategy.slow_sma_period,
                    sma_threshold: config.strategy.sma_threshold,
                    trend_sma_period: config.strategy.trend_sma_period,
                    rsi_threshold: config.strategy.rsi_threshold,
                    trend_divergence_threshold: config.strategy.trend_divergence_threshold,
                    signal_confirmation_bars: config.strategy.signal_confirmation_bars,
                    macd_requires_rising: config.strategy.macd_requires_rising,
                    trend_tolerance_pct: config.strategy.trend_tolerance_pct,
                    macd_min_threshold: config.strategy.macd_min_threshold,
                    adx_threshold: config.strategy.adx_threshold,
                }))
            }
            StrategyMode::TrendRiding => Arc::new(TrendRidingStrategy::new(
                config.strategy.fast_sma_period,
                config.strategy.slow_sma_period,
                config.strategy.sma_threshold,
                config.strategy.trend_riding_exit_buffer_pct,
            )),
            StrategyMode::MeanReversion => Arc::new(MeanReversionStrategy::new(
                config.strategy.mean_reversion_bb_period,
                config.strategy.mean_reversion_rsi_exit,
            )),
            StrategyMode::RegimeAdaptive => Arc::new(TrendRidingStrategy::new(
                config.strategy.fast_sma_period,
                config.strategy.slow_sma_period,
                config.strategy.sma_threshold,
                config.strategy.trend_riding_exit_buffer_pct,
            )),
            StrategyMode::SMC => Arc::new(SMCStrategy::new(
                config.strategy.smc_ob_lookback,
                config.strategy.smc_min_fvg_size_pct,
                config.strategy.smc_volume_multiplier,
            )),
            StrategyMode::VWAP => Arc::new(VWAPStrategy::default()),
            StrategyMode::Breakout => Arc::new(BreakoutStrategy::new(
                config.strategy.breakout_lookback,
                config.strategy.breakout_threshold_pct,
                config.strategy.breakout_volume_mult,
            )),
            StrategyMode::Momentum => Arc::new(MomentumDivergenceStrategy::default()),
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
                let onnx_path = std::path::PathBuf::from("data/ml/model.onnx");
                let bin_path = std::path::PathBuf::from("data/ml/model.bin");

                let predictor: Arc<Box<dyn crate::application::ml::predictor::MLPredictor>> =
                    if onnx_path.exists() {
                        let p =
                            crate::application::ml::onnx_predictor::OnnxPredictor::new(onnx_path);
                        Arc::new(Box::new(p))
                    } else {
                        let p =
                            crate::application::ml::smartcore_predictor::SmartCorePredictor::new(
                                bin_path,
                            );
                        Arc::new(Box::new(p))
                    };

                Arc::new(crate::application::strategies::MLStrategy::new(
                    predictor, 0.0005, // Threshold: 0.05% expected return (Regression Mode)
                ))
            }
        }
    }
}
