use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::strategies::{
    EnsembleStrategy, OrderFlowStrategy, SMCStrategy, StatisticalMomentumStrategy, TradingStrategy,
    ZScoreMeanReversionStrategy,
};
use crate::domain::market::strategy_config::StrategyMode;
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
