pub use super::core::genetic::GeneticOptimizer;
pub use super::core::grid::{GeneBounds, ParameterGrid, decode_genome};
pub use super::core::types::{OptimizationResult, PrefetchedBars, SinglePeriodBars};
pub use super::core::walk_forward::GridSearchOptimizer;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agents::analyst_config::AnalystConfig;
    use crate::config::StrategyMode;
    use crate::domain::config::{RiskConfig, StrategyConfig};
    use crate::domain::trading::fee_model::ConstantFeeModel;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::sync::Arc;

    #[test]
    fn test_objective_score_calculation() {
        let mut result = OptimizationResult {
            params: AnalystConfig {
                strategy: StrategyConfig {
                    strategy_mode: StrategyMode::SMC,
                    fast_sma_period: 20,
                    slow_sma_period: 60,
                    sma_threshold: dec!(0.001),
                    trend_sma_period: 2000,
                    rsi_period: 14,
                    macd_fast_period: 12,
                    macd_slow_period: 26,
                    macd_signal_period: 9,
                    trend_divergence_threshold: dec!(0.005),
                    trailing_stop_atr_multiplier: dec!(3.0),
                    atr_period: 14,
                    rsi_threshold: dec!(65.0),
                    trend_riding_exit_buffer_pct: dec!(0.03),
                    mean_reversion_rsi_exit: dec!(50.0),
                    mean_reversion_bb_period: 20,
                    bb_std_dev: dec!(2.0),
                    ema_fast_period: 50,
                    ema_slow_period: 150,
                    take_profit_pct: dec!(0.05),
                    signal_confirmation_bars: 1,
                    spread_bps: dec!(5.0),
                    min_profit_ratio: dec!(2.0),
                    macd_requires_rising: true,
                    trend_tolerance_pct: dec!(0.0),
                    macd_min_threshold: dec!(0.0),
                    profit_target_multiplier: dec!(1.5),
                    adx_period: 14,
                    adx_threshold: dec!(25.0),
                    smc_ob_lookback: 20,
                    smc_min_fvg_size_pct: dec!(0.005),
                    smc_volume_multiplier: dec!(1.5),
                    risk_appetite_score: None,
                    breakout_lookback: 10,
                    breakout_threshold_pct: dec!(0.002),
                    breakout_volume_mult: dec!(1.1),
                    enable_ml_data_collection: false,
                    stat_momentum_lookback: 10,
                    stat_momentum_threshold: dec!(1.5),
                    stat_momentum_trend_confirmation: true,
                    zscore_lookback: 20,
                    zscore_entry_threshold: dec!(-2.0),
                    zscore_exit_threshold: dec!(0.0),
                    orderflow_ofi_threshold: dec!(0.3),
                    orderflow_stacked_count: 3,
                    orderflow_volume_profile_lookback: 100,
                    ensemble_weights: None,
                    ensemble_voting_threshold: dec!(0.5),
                    ..StrategyConfig::default()
                },
                risk: RiskConfig {
                    max_positions: 5,
                    trade_quantity: Decimal::from(1),
                    order_cooldown_seconds: 300,
                    risk_per_trade_percent: dec!(0.02),
                    max_position_size_pct: dec!(0.1),
                    min_hold_time_minutes: 0,
                    max_loss_per_trade_pct: dec!(-0.05),
                    ..RiskConfig::default()
                },
                fee_model: Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            },
            sharpe_ratio: dec!(2.0),
            total_return: dec!(15.0),
            max_drawdown: dec!(5.0),
            win_rate: dec!(60.0),
            total_trades: 20,
            objective_score: dec!(0.0),
            alpha: dec!(0.01),
            beta: dec!(1.0),
            in_sample_sharpe: None,
            risk_score: None,
        };

        result.calculate_objective_score();

        let expected_penalty = (dec!(30.0) - dec!(20.0)) / dec!(30.0) * dec!(2.0);
        let expected_score = dec!(1.0) + dec!(0.03) + dec!(0.06) - dec!(0.005) - expected_penalty;
        assert!((result.objective_score - expected_score).abs() < dec!(0.01));
    }

    #[test]
    fn test_objective_score_penalizes_high_drawdown() {
        let mut result = OptimizationResult {
            params: AnalystConfig::default(),
            sharpe_ratio: dec!(2.0),
            total_return: dec!(15.0),
            max_drawdown: dec!(-50.0), // 50% drawdown
            win_rate: dec!(60.0),
            total_trades: 50,
            objective_score: dec!(0.0),
            alpha: dec!(0.0),
            beta: dec!(0.0),
            in_sample_sharpe: None,
            risk_score: None,
        };

        result.calculate_objective_score();

        // sharpe = 1.0
        // return = 0.03
        // win = 0.06
        // dd = 0.5 -> squared = 0.25 -> penalty = 0.5
        // expected = 1.0 + 0.03 + 0.06 - 0.5 = 0.59
        assert!((result.objective_score - dec!(0.59)).abs() < dec!(0.01));
    }

    #[test]
    fn test_gene_bounds_includes_modern_params() {
        let grid = ParameterGrid::default();
        let bounds = grid.gene_bounds();
        let (lo, hi) = bounds.stat_momentum_lookback;
        assert!(lo <= hi);
        assert!(bounds.zscore_lookback.0 <= bounds.zscore_lookback.1);
        assert!(bounds.ofi_threshold.0 <= bounds.ofi_threshold.1);
    }

    #[test]
    fn test_generate_combinations_includes_modern_fields() {
        use crate::domain::ports::ExecutionService;
        use tokio::sync::RwLock;

        let grid = ParameterGrid::default();
        let portfolio = Arc::new(RwLock::new(
            crate::domain::trading::portfolio::Portfolio::default(),
        ));
        let market = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());
        let exec_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync> =
            Arc::new(move || {
                Arc::new(crate::infrastructure::mock::MockExecutionService::new(
                    portfolio.clone(),
                ))
            });
        let optimizer = GridSearchOptimizer::new(
            market,
            exec_factory,
            grid,
            StrategyMode::Ensemble,
            dec!(1.5),
        );
        let combos = optimizer.generate_combinations();
        assert!(!combos.is_empty());
        let first = &combos[0];
        assert_eq!(first.strategy.stat_momentum_lookback, 10);
        assert_eq!(first.strategy.zscore_lookback, 20);
        assert_eq!(first.strategy.orderflow_stacked_count, 3);
    }
}
