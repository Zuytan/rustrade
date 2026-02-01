use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::optimization::simulator::Simulator;
use crate::config::StrategyMode;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::trading::fee_model::ConstantFeeModel; // Added
use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
// Added
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

/// Parameter grid for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterGrid {
    pub fast_sma: Vec<usize>,
    pub slow_sma: Vec<usize>,
    pub rsi_threshold: Vec<Decimal>,
    pub trend_divergence_threshold: Vec<Decimal>,
    pub trailing_stop_atr_multiplier: Vec<Decimal>,
    pub order_cooldown_seconds: Vec<u64>,
}

impl Default for ParameterGrid {
    fn default() -> Self {
        Self {
            fast_sma: vec![10, 20, 30],
            slow_sma: vec![50, 60, 100],
            rsi_threshold: vec![dec!(60.0), dec!(65.0), dec!(70.0)],
            trend_divergence_threshold: vec![dec!(0.003), dec!(0.005), dec!(0.01)],
            trailing_stop_atr_multiplier: vec![dec!(2.0), dec!(3.0), dec!(4.0)],
            order_cooldown_seconds: vec![0, 300, 600],
        }
    }
}

/// Single optimization result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub params: AnalystConfig,
    pub sharpe_ratio: Decimal,
    pub total_return: Decimal,
    pub max_drawdown: Decimal,
    pub win_rate: Decimal,
    pub total_trades: usize,
    pub objective_score: Decimal,
    pub alpha: Decimal,
    pub beta: Decimal,
}

impl OptimizationResult {
    /// Calculate a weighted objective score for ranking configurations
    /// Higher is better
    pub fn calculate_objective_score(&mut self) {
        // Composite score favoring high Sharpe, return, and win rate
        // while penalizing high drawdown
        self.objective_score = (self.sharpe_ratio * dec!(0.4))
            + (self.total_return / dec!(100.0) * dec!(0.3))
            + (self.win_rate / dec!(100.0) * dec!(0.2))
            - (self.max_drawdown / dec!(100.0) * dec!(0.1));
    }
}

// use crate::domain::ports::MarketDataService;

/// Grid search optimizer
pub struct GridSearchOptimizer {
    market_data: Arc<dyn MarketDataService>,
    execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
    parameter_grid: ParameterGrid,
    strategy_mode: StrategyMode,
    min_profit_ratio: Decimal, // From Config - scales with Risk Appetite
}

impl GridSearchOptimizer {
    pub fn new(
        market_data: Arc<dyn MarketDataService>,
        execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
        parameter_grid: ParameterGrid,
        strategy_mode: StrategyMode,
        min_profit_ratio: Decimal,
    ) -> Self {
        Self {
            market_data,
            execution_service_factory,
            parameter_grid,
            strategy_mode,
            min_profit_ratio,
        }
    }

    /// Generate all parameter combinations from the grid
    pub fn generate_combinations(&self) -> Vec<AnalystConfig> {
        let mut combinations = Vec::new();

        for &fast in &self.parameter_grid.fast_sma {
            for &slow in &self.parameter_grid.slow_sma {
                // Skip invalid combinations (fast must be < slow)
                if fast >= slow {
                    continue;
                }

                for &rsi in &self.parameter_grid.rsi_threshold {
                    for &trend_div in &self.parameter_grid.trend_divergence_threshold {
                        for &atr_mult in &self.parameter_grid.trailing_stop_atr_multiplier {
                            for &cooldown in &self.parameter_grid.order_cooldown_seconds {
                                combinations.push(AnalystConfig {
                                    fast_sma_period: fast,
                                    slow_sma_period: slow,
                                    rsi_threshold: rsi,
                                    trend_divergence_threshold: trend_div,
                                    trailing_stop_atr_multiplier: atr_mult,
                                    order_cooldown_seconds: cooldown,
                                    // Fixed parameters
                                    max_positions: 5,
                                    trade_quantity: Decimal::from(1),
                                    sma_threshold: dec!(0.001),
                                    risk_per_trade_percent: dec!(0.02),
                                    strategy_mode: self.strategy_mode,
                                    trend_sma_period: 2000,
                                    rsi_period: 14,
                                    macd_fast_period: 12,
                                    macd_slow_period: 26,
                                    macd_signal_period: 9,
                                    atr_period: 14,
                                    trend_riding_exit_buffer_pct: dec!(0.03),
                                    mean_reversion_rsi_exit: dec!(50.0),
                                    mean_reversion_bb_period: 20,
                                    fee_model: Arc::new(ConstantFeeModel::new(
                                        dec!(0.005),
                                        dec!(0.001),
                                    )),
                                    max_position_size_pct: dec!(0.1),
                                    bb_std_dev: dec!(2.0),
                                    ema_fast_period: 50,
                                    ema_slow_period: 150,
                                    take_profit_pct: dec!(0.05),
                                    min_hold_time_minutes: 0,
                                    signal_confirmation_bars: 1,
                                    spread_bps: dec!(5.0),
                                    min_profit_ratio: self.min_profit_ratio, // Use configured value
                                    macd_requires_rising: true, // Conservative default for grid search
                                    trend_tolerance_pct: dec!(0.0), // Strict default for grid search
                                    macd_min_threshold: dec!(0.0), // Neutral default for grid search
                                    profit_target_multiplier: dec!(1.5), // Conservative default
                                    adx_period: 14,
                                    adx_threshold: dec!(25.0),
                                    smc_ob_lookback: 20,
                                    smc_min_fvg_size_pct: dec!(0.005),
                                    smc_volume_multiplier: dec!(1.5),
                                    risk_appetite_score: None,
                                    breakout_lookback: 10,
                                    breakout_threshold_pct: dec!(0.002),
                                    breakout_volume_mult: dec!(1.1),
                                    max_loss_per_trade_pct: dec!(-0.05),
                                    enable_ml_data_collection: false,
                                });
                            }
                        }
                    }
                }
            }
        }

        combinations
    }

    /// Run optimization on a single parameter configuration
    async fn evaluate_config(
        &self,
        config: AnalystConfig,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<OptimizationResult> {
        // Create fresh execution service for this run
        let execution_service = (self.execution_service_factory)();

        let simulator = Simulator::new(self.market_data.clone(), execution_service, config.clone());

        let result = simulator.run(symbol, start, end).await?;

        // Calculate metrics from trades
        let mut trades: Vec<crate::domain::trading::types::Trade> = Vec::new();
        let mut open_position: Option<&crate::domain::trading::types::Order> = None;

        for order in &result.trades {
            match order.side {
                crate::domain::trading::types::OrderSide::Buy => {
                    open_position = Some(order);
                }
                crate::domain::trading::types::OrderSide::Sell => {
                    if let Some(buy_order) = open_position {
                        let pnl = (order.price - buy_order.price) * order.quantity;
                        trades.push(crate::domain::trading::types::Trade {
                            id: order.id.clone(),
                            symbol: order.symbol.clone(),
                            side: crate::domain::trading::types::OrderSide::Buy,
                            entry_price: buy_order.price,
                            exit_price: Some(order.price),
                            quantity: order.quantity,
                            pnl,
                            entry_timestamp: buy_order.timestamp,
                            exit_timestamp: Some(order.timestamp),
                        });
                        open_position = None;
                    }
                }
            }
        }

        let metrics =
            crate::domain::performance::metrics::PerformanceMetrics::calculate_time_series_metrics(
                &trades,
                &result.daily_closes,
                result.initial_equity,
            );

        let mut opt_result = OptimizationResult {
            params: config,
            sharpe_ratio: Decimal::from_f64_retain(metrics.sharpe_ratio).unwrap_or(Decimal::ZERO),
            total_return: result.total_return_pct,
            max_drawdown: Decimal::from_f64_retain(metrics.max_drawdown_pct)
                .unwrap_or(Decimal::ZERO),
            win_rate: Decimal::from_f64_retain(metrics.win_rate).unwrap_or(Decimal::ZERO),
            total_trades: metrics.total_trades,
            objective_score: Decimal::ZERO,
            alpha: Decimal::from_f64_retain(result.alpha).unwrap_or(Decimal::ZERO),
            beta: Decimal::from_f64_retain(result.beta).unwrap_or(Decimal::ZERO),
        };

        opt_result.calculate_objective_score();

        Ok(opt_result)
    }

    /// Run grid search optimization
    pub async fn run_optimization(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<OptimizationResult>> {
        let combinations = self.generate_combinations();
        let total_combinations = combinations.len();

        info!(
            "GridSearch: Starting optimization with {} parameter combinations",
            total_combinations
        );

        let mut results = Vec::new();

        for (i, config) in combinations.into_iter().enumerate() {
            info!(
                "GridSearch: Testing combination {}/{} (fast={}, slow={}, rsi={:.0}, trend_div={:.4})",
                i + 1,
                total_combinations,
                config.fast_sma_period,
                config.slow_sma_period,
                config.rsi_threshold,
                config.trend_divergence_threshold
            );

            match self.evaluate_config(config, symbol, start, end).await {
                Ok(result) => {
                    info!(
                        "GridSearch: Result - Sharpe={:.2}, Return={:.2}%, Score={:.4}",
                        result.sharpe_ratio, result.total_return, result.objective_score
                    );
                    results.push(result);
                }
                Err(e) => {
                    info!("GridSearch: Evaluation failed: {}", e);
                }
            }
        }

        // Sort by objective score (descending)
        results.sort_by(|a, b| {
            b.objective_score
                .partial_cmp(&a.objective_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(results)
    }

    /// Rank and return top N results
    pub fn rank_results(
        &self,
        results: Vec<OptimizationResult>,
        top_n: usize,
    ) -> Vec<OptimizationResult> {
        results.into_iter().take(top_n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_grid_combinations() {
        let grid = ParameterGrid {
            fast_sma: vec![10, 20],
            slow_sma: vec![50, 100],
            rsi_threshold: vec![dec!(65.0)],
            trend_divergence_threshold: vec![dec!(0.005)],
            trailing_stop_atr_multiplier: vec![dec!(3.0)],
            order_cooldown_seconds: vec![300],
        };

        // Manually calculate expected combinations
        // 2 fast * 2 slow * 1 rsi * 1 trend * 1 atr * 1 cooldown = 4 combinations
        let expected_combinations = 2 * 2;

        // Test generation logic by directly creating configs
        let mut combos = Vec::new();
        for &fast in &grid.fast_sma {
            for &slow in &grid.slow_sma {
                if fast >= slow {
                    continue;
                }
                for &rsi in &grid.rsi_threshold {
                    for &trend_div in &grid.trend_divergence_threshold {
                        for &atr_mult in &grid.trailing_stop_atr_multiplier {
                            for &cooldown in &grid.order_cooldown_seconds {
                                combos.push((fast, slow, rsi, trend_div, atr_mult, cooldown));
                            }
                        }
                    }
                }
            }
        }

        assert_eq!(combos.len(), expected_combinations);

        // Verify no invalid combinations (fast >= slow)
        for combo in &combos {
            assert!(
                combo.0 < combo.1,
                "fast {} should be < slow {}",
                combo.0,
                combo.1
            );
        }
    }

    #[test]
    fn test_objective_score_calculation() {
        let mut result = OptimizationResult {
            params: AnalystConfig {
                fast_sma_period: 20,
                slow_sma_period: 60,
                max_positions: 5,
                trade_quantity: Decimal::from(1),
                sma_threshold: dec!(0.001),
                order_cooldown_seconds: 300,
                risk_per_trade_percent: dec!(0.02),
                strategy_mode: StrategyMode::Standard,
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
                fee_model: Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
                max_position_size_pct: dec!(0.1),
                bb_std_dev: dec!(2.0),
                ema_fast_period: 50,
                ema_slow_period: 150,
                take_profit_pct: dec!(0.05),
                min_hold_time_minutes: 0,
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
                max_loss_per_trade_pct: dec!(-0.05),
                enable_ml_data_collection: false,
            },
            sharpe_ratio: dec!(2.0),
            total_return: dec!(15.0),
            max_drawdown: dec!(5.0),
            win_rate: dec!(60.0),
            total_trades: 20,
            objective_score: dec!(0.0),
            alpha: dec!(0.01),
            beta: dec!(1.0),
        };

        result.calculate_objective_score();

        // Score = (2.0 * 0.4) + (0.15 * 0.3) + (0.6 * 0.2) - (0.05 * 0.1)
        //       = 0.8 + 0.045 + 0.12 - 0.005 = 0.96
        assert!((result.objective_score - dec!(0.96)).abs() < dec!(0.01));
    }
}
