use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::optimization::simulator::{BacktestResult, Simulator};
use crate::config::StrategyMode;
use crate::domain::config::StrategyConfig;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::risk::risk_config::RiskConfig;
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use rust_decimal::Decimal;
use std::sync::Arc;

use super::grid::ParameterGrid;
use super::types::{OptimizationResult, PrefetchedBars, SinglePeriodBars};

/// Grid search optimizer
pub struct GridSearchOptimizer {
    pub(crate) market_data: Arc<dyn MarketDataService>,
    pub(crate) execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
    pub(crate) parameter_grid: ParameterGrid,
    pub(crate) strategy_mode: StrategyMode,
    pub(crate) min_profit_ratio: Decimal,
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
        let default_sm_lookback: [usize; 1] = [10];
        let default_sm_threshold: [Decimal; 1] = [rust_decimal_macros::dec!(1.5)];
        let default_zs_lookback: [usize; 1] = [20];
        let default_zs_entry: [Decimal; 1] = [rust_decimal_macros::dec!(-2.0)];
        let default_zs_exit: [Decimal; 1] = [rust_decimal_macros::dec!(0.0)];
        let default_ofi: [Decimal; 1] = [rust_decimal_macros::dec!(0.3)];
        let default_smc_ob: [usize; 1] = [20];
        let default_smc_fvg: [Decimal; 1] = [rust_decimal_macros::dec!(0.005)];

        let sm_lookback = self
            .parameter_grid
            .stat_momentum_lookback
            .as_deref()
            .unwrap_or(&default_sm_lookback);
        let sm_threshold = self
            .parameter_grid
            .stat_momentum_threshold
            .as_deref()
            .unwrap_or(&default_sm_threshold);
        let zs_lookback = self
            .parameter_grid
            .zscore_lookback
            .as_deref()
            .unwrap_or(&default_zs_lookback);
        let zs_entry = self
            .parameter_grid
            .zscore_entry_threshold
            .as_deref()
            .unwrap_or(&default_zs_entry);
        let zs_exit = self
            .parameter_grid
            .zscore_exit_threshold
            .as_deref()
            .unwrap_or(&default_zs_exit);
        let ofi_thr = self
            .parameter_grid
            .ofi_threshold
            .as_deref()
            .unwrap_or(&default_ofi);
        let smc_ob = self
            .parameter_grid
            .smc_ob_lookback
            .as_deref()
            .unwrap_or(&default_smc_ob);
        let smc_fvg = self
            .parameter_grid
            .smc_min_fvg_size_pct
            .as_deref()
            .unwrap_or(&default_smc_fvg);

        for &fast in &self.parameter_grid.fast_sma {
            for &slow in &self.parameter_grid.slow_sma {
                if fast >= slow {
                    continue;
                }
                for &rsi in &self.parameter_grid.rsi_threshold {
                    for &trend_div in &self.parameter_grid.trend_divergence_threshold {
                        for &atr_mult in &self.parameter_grid.trailing_stop_atr_multiplier {
                            for &cooldown in &self.parameter_grid.order_cooldown_seconds {
                                for &stat_mom_lb in sm_lookback {
                                    for &stat_mom_thr in sm_threshold {
                                        for &zs_lb in zs_lookback {
                                            for &zs_en in zs_entry {
                                                for &zs_ex in zs_exit {
                                                    for &ofi in ofi_thr {
                                                        for &ob_lb in smc_ob {
                                                            for &fvg in smc_fvg {
                                                                combinations.push(AnalystConfig {
                                                                    strategy: StrategyConfig {
                                                                        strategy_mode: self.strategy_mode,
                                                                        fast_sma_period: fast,
                                                                        slow_sma_period: slow,
                                                                        trend_sma_period: 200, // Reduced from 2000 for optimization speed
                                                                        sma_threshold: rust_decimal_macros::dec!(0.001),
                                                                        rsi_period: 14,
                                                                        rsi_threshold: rsi,
                                                                        macd_fast_period: 12,
                                                                        macd_slow_period: 26,
                                                                        macd_signal_period: 9,
                                                                        macd_requires_rising: true,
                                                                        macd_min_threshold: rust_decimal_macros::dec!(0.0),
                                                                        ema_fast_period: 50,
                                                                        ema_slow_period: 150,
                                                                        adx_period: 14,
                                                                        adx_threshold: rust_decimal_macros::dec!(25.0),
                                                                        regime_volatility_threshold: rust_decimal_macros::dec!(2.0),
                                                                        bb_std_dev: rust_decimal_macros::dec!(2.0),
                                                                        spread_bps: rust_decimal_macros::dec!(5.0),
                                                                        atr_period: 14,
                                                                        trailing_stop_atr_multiplier: atr_mult,
                                                                        take_profit_pct: rust_decimal_macros::dec!(0.05),
                                                                        profit_target_multiplier: rust_decimal_macros::dec!(1.5),
                                                                        trend_divergence_threshold: trend_div,
                                                                        trend_tolerance_pct: rust_decimal_macros::dec!(0.0),
                                                                        trend_riding_exit_buffer_pct: rust_decimal_macros::dec!(0.03),
                                                                        mean_reversion_rsi_exit: rust_decimal_macros::dec!(50.0),
                                                                        mean_reversion_bb_period: 20,
                                                                        signal_confirmation_bars: 1,
                                                                        strict_sell_htf_confirmation: false,
                                                                        smc_ob_lookback: ob_lb,
                                                                        smc_min_fvg_size_pct: fvg,
                                                                        smc_volume_multiplier: rust_decimal_macros::dec!(1.5),
                                                                        breakout_lookback: 10,
                                                                        breakout_threshold_pct: rust_decimal_macros::dec!(0.002),
                                                                        breakout_volume_mult: rust_decimal_macros::dec!(1.1),
                                                                        stat_momentum_lookback: stat_mom_lb,
                                                                        stat_momentum_threshold: stat_mom_thr,
                                                                        stat_momentum_trend_confirmation: true,
                                                                        zscore_lookback: zs_lb,
                                                                        zscore_entry_threshold: zs_en,
                                                                        zscore_exit_threshold: zs_ex,
                                                                        orderflow_ofi_threshold: ofi,
                                                                        orderflow_stacked_count: 3,
                                                                        orderflow_volume_profile_lookback: 100,
                                                                        ensemble_weights: None,
                                                                        ensemble_voting_threshold: rust_decimal_macros::dec!(0.5),
                                                                        primary_timeframe: crate::domain::market::timeframe::Timeframe::OneMin,
                                                                        enabled_timeframes: vec![crate::domain::market::timeframe::Timeframe::OneMin],
                                                                        trend_timeframe: crate::domain::market::timeframe::Timeframe::OneHour,
                                                                        enable_ml_data_collection: false,
                                                                        risk_appetite_score: None,
                                                                        snn_activation_threshold: rust_decimal_macros::dec!(0.8),
                                                                        snn_encoder_threshold: rust_decimal_macros::dec!(0.01),
                                                                        snn_surrogate_model_path: "models/snn/snn_surrogate_model.json".to_string(),
                                                                        snn_surrogate_window_size: 50,
                                                                        ensemble_include_snn: false,
                                                                        ensemble_snn_weight: rust_decimal_macros::dec!(0.3),
                                                                        min_profit_ratio: self.min_profit_ratio,
                                                                    },
                                                                    risk: RiskConfig {
                                                                        max_positions: 5,
                                                                        risk_per_trade_percent: rust_decimal_macros::dec!(0.02),
                                                                        trade_quantity: Decimal::from(1),
                                                                        order_cooldown_seconds: cooldown,
                                                                        max_position_size_pct: rust_decimal_macros::dec!(0.1),
                                                                        min_hold_time_minutes: 0,
                                                                        max_loss_per_trade_pct: rust_decimal_macros::dec!(-0.05),
                                                                        ..Default::default()
                                                                    },
                                                                    fee_model: Arc::new(crate::domain::trading::fee_model::ConstantFeeModel::new(rust_decimal_macros::dec!(0.005), rust_decimal_macros::dec!(0.001))),
                                                                });
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        combinations
    }

    pub fn rank_results(
        &self,
        mut results: Vec<OptimizationResult>,
        top_n: usize,
    ) -> Vec<OptimizationResult> {
        results.sort_by(|a, b| {
            b.objective_score
                .partial_cmp(&a.objective_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(top_n);
        results
    }

    /// One evaluation with pre-fetched bars (for parallel run).
    pub(crate) async fn evaluate_one_with_bars(
        market_data: Arc<dyn MarketDataService>,
        execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
        config: AnalystConfig,
        symbol: String,
        prefetched: Arc<PrefetchedBars>,
    ) -> Result<(OptimizationResult, OptimizationResult)> {
        let exec_train = (execution_service_factory)();
        let sim_train = Simulator::new(market_data.clone(), exec_train, config.clone());
        let result_train = sim_train
            .run_with_bars(
                &symbol,
                &prefetched.train_bars,
                prefetched.train_start,
                prefetched.train_end,
                Some(prefetched.spy_train.clone()),
            )
            .await?;
        let exec_test = (execution_service_factory)();
        let sim_test = Simulator::new(market_data.clone(), exec_test, config.clone());
        let result_test = sim_test
            .run_with_bars(
                &symbol,
                &prefetched.test_bars,
                prefetched.test_start,
                prefetched.test_end,
                Some(prefetched.spy_test.clone()),
            )
            .await?;
        let train_opt = backtest_result_to_opt_result_impl(config.clone(), result_train);
        let test_opt = backtest_result_to_opt_result_impl(config, result_test);
        Ok((train_opt, test_opt))
    }

    /// One backtest over full period (single-period mode, for parallel run).
    pub(crate) async fn evaluate_one_single_period(
        market_data: Arc<dyn MarketDataService>,
        execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
        config: AnalystConfig,
        symbol: String,
        prefetched: Arc<SinglePeriodBars>,
    ) -> Result<OptimizationResult> {
        let exec = (execution_service_factory)();
        let sim = Simulator::new(market_data, exec, config.clone());
        let result = sim
            .run_with_bars(
                &symbol,
                &prefetched.bars,
                prefetched.start,
                prefetched.end,
                Some(prefetched.spy_bars.clone()),
            )
            .await?;
        Ok(backtest_result_to_opt_result_impl(config, result))
    }

    /// Run grid search optimization.
    pub async fn run_optimization(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        train_ratio: f64,
    ) -> Result<Vec<OptimizationResult>> {
        const PARALLEL_WORKERS: usize = 4;
        let _total_combinations = self.generate_combinations().len();

        let bars = self
            .market_data
            .get_historical_bars(symbol, start, end, "1Min")
            .await?;
        if bars.is_empty() {
            return Err(anyhow::anyhow!("No candles found for {}", symbol));
        }

        let spy_bars = self
            .market_data
            .get_historical_bars("SPY", start, end, "1Day")
            .await
            .unwrap_or_default();

        let configs = self.generate_combinations();

        if train_ratio >= 1.0 {
            let prefetched = Arc::new(SinglePeriodBars {
                bars,
                start,
                end,
                spy_bars,
            });

            let completed: Vec<Result<OptimizationResult>> = stream::iter(configs)
                .map(|config| {
                    let market_data = self.market_data.clone();
                    let exec_factory = self.execution_service_factory.clone();
                    let symbol = symbol.to_string();
                    let prefetched = prefetched.clone();
                    async move {
                        Self::evaluate_one_single_period(
                            market_data,
                            exec_factory,
                            config,
                            symbol,
                            prefetched,
                        )
                        .await
                    }
                })
                .buffer_unordered(PARALLEL_WORKERS)
                .collect()
                .await;

            let mut final_results = Vec::new();
            for res in completed.into_iter().flatten() {
                if res.total_trades > 0 {
                    final_results.push(res);
                }
            }
            final_results.sort_by(|a, b| {
                b.objective_score
                    .partial_cmp(&a.objective_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(final_results)
        } else {
            let split_idx = (bars.len() as f64 * train_ratio) as usize;
            let (train_bars, test_bars) = bars.split_at(split_idx);
            let train_end_ts = train_bars.last().map(|b| b.timestamp).unwrap_or(0);
            let train_end = DateTime::from_timestamp(train_end_ts, 0).unwrap_or(Utc::now());
            let test_start = train_end;

            let (spy_train, spy_test) = if spy_bars.len() >= bars.len() {
                let s_split = (spy_bars.len() as f64 * train_ratio) as usize;
                let (s_tr, s_te) = spy_bars.split_at(s_split);
                (s_tr.to_vec(), s_te.to_vec())
            } else {
                (vec![], vec![])
            };

            let prefetched = Arc::new(PrefetchedBars {
                train_bars: train_bars.to_vec(),
                train_start: start,
                train_end,
                test_bars: test_bars.to_vec(),
                test_start,
                test_end: end,
                spy_train,
                spy_test,
            });

            let completed: Vec<Result<(OptimizationResult, OptimizationResult)>> =
                stream::iter(configs)
                    .map(|config| {
                        let market_data = self.market_data.clone();
                        let exec_factory = self.execution_service_factory.clone();
                        let symbol = symbol.to_string();
                        let prefetched = prefetched.clone();
                        async move {
                            Self::evaluate_one_with_bars(
                                market_data,
                                exec_factory,
                                config,
                                symbol,
                                prefetched,
                            )
                            .await
                        }
                    })
                    .buffer_unordered(PARALLEL_WORKERS)
                    .collect()
                    .await;

            let mut final_results = Vec::new();
            for (train_opt, mut test_opt) in completed.into_iter().flatten() {
                if test_opt.total_trades > 0 {
                    test_opt.in_sample_sharpe = Some(train_opt.sharpe_ratio);
                    final_results.push(test_opt);
                }
            }
            final_results.sort_by(|a, b| {
                b.objective_score
                    .partial_cmp(&a.objective_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(final_results)
        }
    }
}

pub(crate) async fn run_single_period_eval(
    market_data: Arc<dyn MarketDataService>,
    execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
    config: AnalystConfig,
    symbol: String,
    prefetched: Arc<SinglePeriodBars>,
) -> Result<OptimizationResult> {
    let exec = (execution_service_factory)();
    let sim = Simulator::new(market_data, exec, config.clone());
    let result = sim
        .run_with_bars(
            &symbol,
            &prefetched.bars,
            prefetched.start,
            prefetched.end,
            Some(prefetched.spy_bars.clone()),
        )
        .await?;
    Ok(backtest_result_to_opt_result_impl(config, result))
}

pub(crate) fn backtest_result_to_opt_result_impl(
    config: AnalystConfig,
    result: BacktestResult,
) -> OptimizationResult {
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
                        strategy_used: None,
                        regime_detected: None,
                        entry_reason: None,
                        exit_reason: None,
                        slippage: None,
                        fees: rust_decimal::Decimal::ZERO,
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
        max_drawdown: metrics.max_drawdown_pct,
        win_rate: Decimal::from_f64_retain(metrics.win_rate).unwrap_or(Decimal::ZERO),
        total_trades: metrics.total_trades,
        objective_score: Decimal::ZERO,
        alpha: Decimal::from_f64_retain(result.alpha).unwrap_or(Decimal::ZERO),
        beta: Decimal::from_f64_retain(result.beta).unwrap_or(Decimal::ZERO),
        in_sample_sharpe: None,
        risk_score: None,
    };

    opt_result.calculate_objective_score();
    opt_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_rank_results() {
        let market_data = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());
        let exec_factory = Arc::new(|| -> Arc<dyn ExecutionService> {
            let p_lock = Arc::new(tokio::sync::RwLock::new(
                crate::domain::trading::portfolio::Portfolio::new(),
            ));
            Arc::new(crate::infrastructure::mock::MockExecutionService::new(
                p_lock,
            ))
        });

        let optimizer = GridSearchOptimizer::new(
            market_data,
            exec_factory,
            ParameterGrid {
                fast_sma: vec![5],
                slow_sma: vec![10],
                rsi_threshold: vec![dec!(60.0)],
                trend_divergence_threshold: vec![dec!(0.005)],
                trailing_stop_atr_multiplier: vec![dec!(3.0)],
                order_cooldown_seconds: vec![0],
                stat_momentum_lookback: None,
                stat_momentum_threshold: None,
                zscore_lookback: None,
                zscore_entry_threshold: None,
                zscore_exit_threshold: None,
                ofi_threshold: None,
                smc_ob_lookback: None,
                smc_min_fvg_size_pct: None,
            },
            StrategyMode::SMC,
            dec!(0.001),
        );

        let res1 = OptimizationResult {
            params: AnalystConfig::default(),
            sharpe_ratio: dec!(1.5),
            total_return: dec!(10.0),
            max_drawdown: dec!(5.0),
            win_rate: dec!(0.6),
            total_trades: 10,
            objective_score: dec!(1.5),
            alpha: dec!(0.1),
            beta: dec!(1.0),
            in_sample_sharpe: None,
            risk_score: None,
        };
        let res2 = OptimizationResult {
            params: AnalystConfig::default(),
            sharpe_ratio: dec!(2.5),
            total_return: dec!(20.0),
            max_drawdown: dec!(3.0),
            win_rate: dec!(0.7),
            total_trades: 15,
            objective_score: dec!(2.5),
            alpha: dec!(0.2),
            beta: dec!(0.9),
            in_sample_sharpe: None,
            risk_score: None,
        };
        let res3 = OptimizationResult {
            params: AnalystConfig::default(),
            sharpe_ratio: dec!(0.5),
            total_return: dec!(5.0),
            max_drawdown: dec!(10.0),
            win_rate: dec!(0.4),
            total_trades: 5,
            objective_score: dec!(0.5),
            alpha: dec!(0.0),
            beta: dec!(1.2),
            in_sample_sharpe: None,
            risk_score: None,
        };

        let ranked = optimizer.rank_results(vec![res1.clone(), res2.clone(), res3.clone()], 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].objective_score, dec!(2.5));
        assert_eq!(ranked[1].objective_score, dec!(1.5));
    }

    #[test]
    fn test_generate_combinations() {
        let market_data = Arc::new(crate::infrastructure::mock::MockMarketDataService::new());
        let exec_factory = Arc::new(|| -> Arc<dyn ExecutionService> {
            let p_lock = Arc::new(tokio::sync::RwLock::new(
                crate::domain::trading::portfolio::Portfolio::new(),
            ));
            Arc::new(crate::infrastructure::mock::MockExecutionService::new(
                p_lock,
            ))
        });

        let optimizer = GridSearchOptimizer::new(
            market_data,
            exec_factory,
            ParameterGrid {
                fast_sma: vec![5, 10, 20],
                slow_sma: vec![10, 15, 30],
                rsi_threshold: vec![dec!(60.0)],
                trend_divergence_threshold: vec![dec!(0.005)],
                trailing_stop_atr_multiplier: vec![dec!(3.0)],
                order_cooldown_seconds: vec![0],
                stat_momentum_lookback: None,
                stat_momentum_threshold: None,
                zscore_lookback: None,
                zscore_entry_threshold: None,
                zscore_exit_threshold: None,
                ofi_threshold: None,
                smc_ob_lookback: None,
                smc_min_fvg_size_pct: None,
            },
            StrategyMode::SMC,
            dec!(0.001),
        );

        let combinations = optimizer.generate_combinations();
        assert!(!combinations.is_empty());

        for combo in &combinations {
            assert!(combo.strategy.fast_sma_period < combo.strategy.slow_sma_period);
        }
    }

    #[test]
    fn test_backtest_result_to_opt_result_mapping() {
        let config = AnalystConfig::default();
        let result = BacktestResult {
            trades: vec![],
            final_equity: dec!(10000.0),
            initial_equity: dec!(10000.0),
            total_return_pct: dec!(0.0),
            buy_and_hold_return_pct: dec!(0.0),
            daily_closes: vec![],
            alpha: 0.0,
            beta: 1.0,
            benchmark_correlation: 0.0,
        };

        let opt_result = backtest_result_to_opt_result_impl(config, result);
        assert_eq!(opt_result.total_return, dec!(0.0));
        assert_eq!(opt_result.alpha, dec!(0.0));
        assert_eq!(opt_result.beta, dec!(1.0));
    }
}
