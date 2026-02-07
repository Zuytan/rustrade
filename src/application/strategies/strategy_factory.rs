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
                config.smc_volume_multiplier,
            )),
            StrategyMode::VWAP => Arc::new(VWAPStrategy::default()),
            StrategyMode::Breakout => Arc::new(BreakoutStrategy::new(
                config.breakout_lookback,
                config.breakout_threshold_pct,
                config.breakout_volume_mult,
            )),
            StrategyMode::Momentum => Arc::new(MomentumDivergenceStrategy::default()),
            StrategyMode::Ensemble => Arc::new(EnsembleStrategy::modern_ensemble(config)),
            // Modern statistical/microstructure strategies (params from config)
            StrategyMode::ZScoreMR => Arc::new(ZScoreMeanReversionStrategy::new(
                config.zscore_lookback,
                config.zscore_entry_threshold,
                config.zscore_exit_threshold,
            )),
            StrategyMode::StatMomentum => Arc::new(StatisticalMomentumStrategy::new(
                config.stat_momentum_lookback,
                config.stat_momentum_threshold,
                config.stat_momentum_trend_confirmation,
            )),
            StrategyMode::OrderFlow => Arc::new(OrderFlowStrategy::new(
                config.orderflow_ofi_threshold,
                config.orderflow_stacked_count,
                config.orderflow_volume_profile_lookback,
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
