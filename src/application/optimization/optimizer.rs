use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::optimization::simulator::{BacktestResult, Simulator};
use crate::config::StrategyMode;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::risk::optimal_parameters::{AssetType, OptimalParameters};
use crate::domain::risk::risk_appetite::{RiskAppetite, RiskProfile};
use crate::domain::trading::fee_model::ConstantFeeModel;
use crate::domain::trading::types::Candle;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};

/// Parameter grid for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterGrid {
    pub fast_sma: Vec<usize>,
    pub slow_sma: Vec<usize>,
    pub rsi_threshold: Vec<Decimal>,
    pub trend_divergence_threshold: Vec<Decimal>,
    pub trailing_stop_atr_multiplier: Vec<Decimal>,
    pub order_cooldown_seconds: Vec<u64>,
    /// Modern strategy params (optional for backward compat)
    pub stat_momentum_lookback: Option<Vec<usize>>,
    pub stat_momentum_threshold: Option<Vec<Decimal>>,
    pub zscore_lookback: Option<Vec<usize>>,
    pub zscore_entry_threshold: Option<Vec<Decimal>>,
    pub zscore_exit_threshold: Option<Vec<Decimal>>,
    pub ofi_threshold: Option<Vec<Decimal>>,
    pub smc_ob_lookback: Option<Vec<usize>>,
    pub smc_min_fvg_size_pct: Option<Vec<Decimal>>,
}

impl Default for ParameterGrid {
    /// Balanced default grid: good coverage with ~400–600 combinations (after fast<slow filter).
    /// Optimized for walk-forward OOS; use a TOML grid for finer or profile-specific search.
    fn default() -> Self {
        Self {
            fast_sma: vec![10, 15, 20, 25],
            slow_sma: vec![50, 60, 80, 100],
            rsi_threshold: vec![dec!(58.0), dec!(62.0), dec!(66.0), dec!(70.0)],
            trend_divergence_threshold: vec![dec!(0.002), dec!(0.004), dec!(0.006), dec!(0.01)],
            trailing_stop_atr_multiplier: vec![dec!(2.0), dec!(2.5), dec!(3.0), dec!(4.0)],
            order_cooldown_seconds: vec![0, 300, 600],
            stat_momentum_lookback: None,
            stat_momentum_threshold: None,
            zscore_lookback: None,
            zscore_entry_threshold: None,
            zscore_exit_threshold: None,
            ofi_threshold: None,
            smc_ob_lookback: None,
            smc_min_fvg_size_pct: None,
        }
    }
}

impl ParameterGrid {
    /// Bounds for genetic algorithm (min/max per dimension). Uses grid extremes or defaults.
    pub fn gene_bounds(&self) -> GeneBounds {
        let fast = (
            self.fast_sma.iter().min().copied().unwrap_or(5) as f64,
            self.fast_sma.iter().max().copied().unwrap_or(50) as f64,
        );
        let slow = (
            self.slow_sma.iter().min().copied().unwrap_or(30) as f64,
            self.slow_sma.iter().max().copied().unwrap_or(150) as f64,
        );
        let rsi = (
            self.rsi_threshold
                .iter()
                .min_by(|a, b| a.cmp(b))
                .and_then(|d| d.to_f64())
                .unwrap_or(50.0),
            self.rsi_threshold
                .iter()
                .max_by(|a, b| a.cmp(b))
                .and_then(|d| d.to_f64())
                .unwrap_or(75.0),
        );
        let trend_div = (
            self.trend_divergence_threshold
                .iter()
                .min_by(|a, b| a.cmp(b))
                .and_then(|d| d.to_f64())
                .unwrap_or(0.001),
            self.trend_divergence_threshold
                .iter()
                .max_by(|a, b| a.cmp(b))
                .and_then(|d| d.to_f64())
                .unwrap_or(0.02),
        );
        let atr_mult = (
            self.trailing_stop_atr_multiplier
                .iter()
                .min_by(|a, b| a.cmp(b))
                .and_then(|d| d.to_f64())
                .unwrap_or(1.5),
            self.trailing_stop_atr_multiplier
                .iter()
                .max_by(|a, b| a.cmp(b))
                .and_then(|d| d.to_f64())
                .unwrap_or(5.0),
        );
        let cooldown = (
            self.order_cooldown_seconds
                .iter()
                .min()
                .copied()
                .unwrap_or(0) as f64,
            self.order_cooldown_seconds
                .iter()
                .max()
                .copied()
                .unwrap_or(900) as f64,
        );
        let stat_mom_lb = (
            self.stat_momentum_lookback
                .as_ref()
                .and_then(|v| v.iter().min().copied())
                .unwrap_or(5) as f64,
            self.stat_momentum_lookback
                .as_ref()
                .and_then(|v| v.iter().max().copied())
                .unwrap_or(30) as f64,
        );
        let stat_mom_thr = (
            self.stat_momentum_threshold
                .as_ref()
                .and_then(|v| v.iter().min_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(1.0),
            self.stat_momentum_threshold
                .as_ref()
                .and_then(|v| v.iter().max_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(3.0),
        );
        let zscore_lb = (
            self.zscore_lookback
                .as_ref()
                .and_then(|v| v.iter().min().copied())
                .unwrap_or(10) as f64,
            self.zscore_lookback
                .as_ref()
                .and_then(|v| v.iter().max().copied())
                .unwrap_or(40) as f64,
        );
        let zscore_entry = (
            self.zscore_entry_threshold
                .as_ref()
                .and_then(|v| v.iter().max_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(-3.0),
            self.zscore_entry_threshold
                .as_ref()
                .and_then(|v| v.iter().min_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(-1.0),
        );
        let zscore_exit = (
            self.zscore_exit_threshold
                .as_ref()
                .and_then(|v| v.iter().min_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(-0.5),
            self.zscore_exit_threshold
                .as_ref()
                .and_then(|v| v.iter().max_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(0.5),
        );
        let ofi = (
            self.ofi_threshold
                .as_ref()
                .and_then(|v| v.iter().min_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(0.2),
            self.ofi_threshold
                .as_ref()
                .and_then(|v| v.iter().max_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(0.5),
        );
        let smc_ob = (
            self.smc_ob_lookback
                .as_ref()
                .and_then(|v| v.iter().min().copied())
                .unwrap_or(10) as f64,
            self.smc_ob_lookback
                .as_ref()
                .and_then(|v| v.iter().max().copied())
                .unwrap_or(40) as f64,
        );
        let smc_fvg = (
            self.smc_min_fvg_size_pct
                .as_ref()
                .and_then(|v| v.iter().min_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(0.001),
            self.smc_min_fvg_size_pct
                .as_ref()
                .and_then(|v| v.iter().max_by(|a, b| a.cmp(b)).and_then(|d| d.to_f64()))
                .unwrap_or(0.02),
        );
        GeneBounds {
            fast_sma: fast,
            slow_sma: slow,
            rsi_threshold: rsi,
            trend_divergence_threshold: trend_div,
            trailing_stop_atr_multiplier: atr_mult,
            order_cooldown_seconds: cooldown,
            stat_momentum_lookback: stat_mom_lb,
            stat_momentum_threshold: stat_mom_thr,
            zscore_lookback: zscore_lb,
            zscore_entry_threshold: zscore_entry,
            zscore_exit_threshold: zscore_exit,
            ofi_threshold: ofi,
            smc_ob_lookback: smc_ob,
            smc_min_fvg_size_pct: smc_fvg,
        }
    }
}

/// Bounds for each gene (min, max) used by the genetic algorithm.
#[derive(Debug, Clone)]
pub struct GeneBounds {
    pub fast_sma: (f64, f64),
    pub slow_sma: (f64, f64),
    pub rsi_threshold: (f64, f64),
    pub trend_divergence_threshold: (f64, f64),
    pub trailing_stop_atr_multiplier: (f64, f64),
    pub order_cooldown_seconds: (f64, f64),
    pub stat_momentum_lookback: (f64, f64),
    pub stat_momentum_threshold: (f64, f64),
    pub zscore_lookback: (f64, f64),
    pub zscore_entry_threshold: (f64, f64),
    pub zscore_exit_threshold: (f64, f64),
    pub ofi_threshold: (f64, f64),
    pub smc_ob_lookback: (f64, f64),
    pub smc_min_fvg_size_pct: (f64, f64),
}

/// Genome: 14 genes in [0, 1], decoded with GeneBounds to AnalystConfig params.
fn decode_genome(
    genome: &[f64; 14],
    bounds: &GeneBounds,
    strategy_mode: StrategyMode,
    min_profit_ratio: Decimal,
) -> AnalystConfig {
    let lerp = |v: f64, (lo, hi): (f64, f64)| lo + v * (hi - lo);
    let fast = lerp(genome[0].clamp(0.0, 1.0), bounds.fast_sma).round() as usize;
    let slow_raw = lerp(genome[1].clamp(0.0, 1.0), bounds.slow_sma).round() as usize;
    let slow = slow_raw.max(fast + 1);
    let rsi = Decimal::from_f64_retain(lerp(genome[2].clamp(0.0, 1.0), bounds.rsi_threshold))
        .unwrap_or(dec!(60.0));
    let trend_div = Decimal::from_f64_retain(lerp(
        genome[3].clamp(0.0, 1.0),
        bounds.trend_divergence_threshold,
    ))
    .unwrap_or(dec!(0.005));
    let atr_mult = Decimal::from_f64_retain(lerp(
        genome[4].clamp(0.0, 1.0),
        bounds.trailing_stop_atr_multiplier,
    ))
    .unwrap_or(dec!(3.0));
    let cooldown = lerp(genome[5].clamp(0.0, 1.0), bounds.order_cooldown_seconds).round() as u64;
    let stat_mom_lookback =
        lerp(genome[6].clamp(0.0, 1.0), bounds.stat_momentum_lookback).round() as usize;
    let stat_mom_threshold = Decimal::from_f64_retain(lerp(
        genome[7].clamp(0.0, 1.0),
        bounds.stat_momentum_threshold,
    ))
    .unwrap_or(dec!(1.5));
    let zscore_lookback = lerp(genome[8].clamp(0.0, 1.0), bounds.zscore_lookback).round() as usize;
    let zscore_entry = Decimal::from_f64_retain(lerp(
        genome[9].clamp(0.0, 1.0),
        bounds.zscore_entry_threshold,
    ))
    .unwrap_or(dec!(-2.0));
    let zscore_exit = Decimal::from_f64_retain(lerp(
        genome[10].clamp(0.0, 1.0),
        bounds.zscore_exit_threshold,
    ))
    .unwrap_or(dec!(0.0));
    let ofi_threshold =
        Decimal::from_f64_retain(lerp(genome[11].clamp(0.0, 1.0), bounds.ofi_threshold))
            .unwrap_or(dec!(0.3));
    let smc_ob_lookback = lerp(genome[12].clamp(0.0, 1.0), bounds.smc_ob_lookback).round() as usize;
    let smc_min_fvg = Decimal::from_f64_retain(lerp(
        genome[13].clamp(0.0, 1.0),
        bounds.smc_min_fvg_size_pct,
    ))
    .unwrap_or(dec!(0.005));
    AnalystConfig {
        fast_sma_period: fast,
        slow_sma_period: slow,
        rsi_threshold: rsi,
        trend_divergence_threshold: trend_div,
        trailing_stop_atr_multiplier: atr_mult,
        order_cooldown_seconds: cooldown,
        max_positions: 5,
        trade_quantity: Decimal::from(1),
        sma_threshold: dec!(0.001),
        risk_per_trade_percent: dec!(0.02),
        strategy_mode,
        trend_sma_period: 2000,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        atr_period: 14,
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
        min_profit_ratio,
        macd_requires_rising: true,
        trend_tolerance_pct: dec!(0.0),
        macd_min_threshold: dec!(0.0),
        profit_target_multiplier: dec!(1.5),
        adx_period: 14,
        adx_threshold: dec!(25.0),
        smc_ob_lookback,
        smc_min_fvg_size_pct: smc_min_fvg,
        smc_volume_multiplier: dec!(1.5),
        risk_appetite_score: None,
        breakout_lookback: 10,
        breakout_threshold_pct: dec!(0.002),
        breakout_volume_mult: dec!(1.1),
        max_loss_per_trade_pct: dec!(-0.05),
        enable_ml_data_collection: false,
        stat_momentum_lookback: stat_mom_lookback,
        stat_momentum_threshold: stat_mom_threshold,
        stat_momentum_trend_confirmation: true,
        zscore_lookback,
        zscore_entry_threshold: zscore_entry,
        zscore_exit_threshold: zscore_exit,
        orderflow_ofi_threshold: ofi_threshold,
        orderflow_stacked_count: 3,
        orderflow_volume_profile_lookback: 100,
        ensemble_weights: None,
        ensemble_voting_threshold: dec!(0.5),
    }
}

/// Single optimization result. In walk-forward mode, sharpe_ratio is OOS and in_sample_sharpe is set.
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
    /// When set, optimization used train/test split; sharpe_ratio is OOS, this is in-sample.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_sample_sharpe: Option<Decimal>,
    /// Risk score (1-9) when optimization was run with --risk-score. Enables loading best params per risk in benchmark.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_score: Option<u8>,
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

    /// Build OptimalParameters for persistence (e.g. ~/.rustrade/optimal_parameters.json) from this result.
    /// Call with the risk_score and asset_type used for the run, and the symbol optimized.
    pub fn to_optimal_parameters(
        &self,
        risk_score: u8,
        asset_type: AssetType,
        symbol_used: String,
    ) -> OptimalParameters {
        let profile = RiskAppetite::new(risk_score)
            .map(|r| r.profile())
            .unwrap_or(RiskProfile::Balanced);
        OptimalParameters::new(
            asset_type,
            profile,
            self.params.fast_sma_period,
            self.params.slow_sma_period,
            self.params.rsi_threshold,
            self.params.trailing_stop_atr_multiplier,
            self.params.trend_divergence_threshold,
            self.params.order_cooldown_seconds,
            symbol_used,
            self.sharpe_ratio,
            self.total_return,
            self.max_drawdown,
            self.win_rate,
            self.total_trades,
        )
        .with_risk_score(risk_score)
    }

    /// Build OptimalParameters for a given profile without risk_score (used when saving "for all" without --risk-score).
    pub fn to_optimal_parameters_for_profile(
        &self,
        profile: RiskProfile,
        asset_type: AssetType,
        symbol_used: String,
    ) -> OptimalParameters {
        OptimalParameters::new(
            asset_type,
            profile,
            self.params.fast_sma_period,
            self.params.slow_sma_period,
            self.params.rsi_threshold,
            self.params.trailing_stop_atr_multiplier,
            self.params.trend_divergence_threshold,
            self.params.order_cooldown_seconds,
            symbol_used,
            self.sharpe_ratio,
            self.total_return,
            self.max_drawdown,
            self.win_rate,
            self.total_trades,
        )
    }
}

// use crate::domain::ports::MarketDataService;

/// Pre-fetched bars for walk-forward optimization (avoids repeated API calls).
struct PrefetchedBars {
    train_bars: Vec<Candle>,
    train_start: DateTime<Utc>,
    train_end: DateTime<Utc>,
    test_bars: Vec<Candle>,
    test_start: DateTime<Utc>,
    test_end: DateTime<Utc>,
    spy_train: Vec<Candle>,
    spy_test: Vec<Candle>,
}

/// Pre-fetched bars for single-period optimization (one backtest over full range).
struct SinglePeriodBars {
    bars: Vec<Candle>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    spy_bars: Vec<Candle>,
}

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
        let default_sm_lookback: [usize; 1] = [10];
        let default_sm_threshold: [Decimal; 1] = [dec!(1.5)];
        let default_zs_lookback: [usize; 1] = [20];
        let default_zs_entry: [Decimal; 1] = [dec!(-2.0)];
        let default_zs_exit: [Decimal; 1] = [dec!(0.0)];
        let default_ofi: [Decimal; 1] = [dec!(0.3)];
        let default_smc_ob: [usize; 1] = [20];
        let default_smc_fvg: [Decimal; 1] = [dec!(0.005)];

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
                                                                    fast_sma_period: fast,
                                                                    slow_sma_period: slow,
                                                                    rsi_threshold: rsi,
                                                                    trend_divergence_threshold: trend_div,
                                                                    trailing_stop_atr_multiplier: atr_mult,
                                                                    order_cooldown_seconds: cooldown,
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
                                                                    fee_model: Arc::new(
                                                                        ConstantFeeModel::new(
                                                                            dec!(0.005),
                                                                            dec!(0.001),
                                                                        ),
                                                                    ),
                                                                    max_position_size_pct: dec!(0.1),
                                                                    bb_std_dev: dec!(2.0),
                                                                    ema_fast_period: 50,
                                                                    ema_slow_period: 150,
                                                                    take_profit_pct: dec!(0.05),
                                                                    min_hold_time_minutes: 0,
                                                                    signal_confirmation_bars: 1,
                                                                    spread_bps: dec!(5.0),
                                                                    min_profit_ratio: self.min_profit_ratio,
                                                                    macd_requires_rising: true,
                                                                    trend_tolerance_pct: dec!(0.0),
                                                                    macd_min_threshold: dec!(0.0),
                                                                    profit_target_multiplier: dec!(1.5),
                                                                    adx_period: 14,
                                                                    adx_threshold: dec!(25.0),
                                                                    smc_ob_lookback: ob_lb,
                                                                    smc_min_fvg_size_pct: fvg,
                                                                    smc_volume_multiplier: dec!(1.5),
                                                                    risk_appetite_score: None,
                                                                    breakout_lookback: 10,
                                                                    breakout_threshold_pct: dec!(0.002),
                                                                    breakout_volume_mult: dec!(1.1),
                                                                    max_loss_per_trade_pct: dec!(-0.05),
                                                                    enable_ml_data_collection: false,
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
                                                                    ensemble_voting_threshold: dec!(0.5),
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

    /// Build OptimizationResult from a backtest result (shared by evaluate_config and evaluate_config_with_bars).
    fn backtest_result_to_opt_result(
        config: AnalystConfig,
        result: BacktestResult,
    ) -> OptimizationResult {
        backtest_result_to_opt_result_impl(config, result)
    }
}

/// Run one single-period backtest (shared by grid and genetic).
async fn run_single_period_eval(
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

/// Shared: build OptimizationResult from backtest (used by grid and genetic).
fn backtest_result_to_opt_result_impl(
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
        max_drawdown: Decimal::from_f64_retain(metrics.max_drawdown_pct).unwrap_or(Decimal::ZERO),
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

impl GridSearchOptimizer {
    /// Run optimization on a single parameter configuration (fetches bars each time).
    /// Kept for single-period or external use; run_optimization uses evaluate_config_with_bars.
    #[allow(dead_code)]
    async fn evaluate_config(
        &self,
        config: AnalystConfig,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<OptimizationResult> {
        let execution_service = (self.execution_service_factory)();
        let simulator = Simulator::new(self.market_data.clone(), execution_service, config.clone());
        let result = simulator.run(symbol, start, end).await?;
        Ok(Self::backtest_result_to_opt_result(config, result))
    }

    /// Evaluate one config using pre-fetched bars (train + test). Returns (train_result, test_result).
    /// Kept for single-threaded use; run_optimization uses evaluate_one_with_bars in parallel.
    #[allow(dead_code)]
    async fn evaluate_config_with_bars(
        &self,
        config: AnalystConfig,
        symbol: &str,
        prefetched: &PrefetchedBars,
    ) -> Result<(OptimizationResult, OptimizationResult)> {
        let exec_train = (self.execution_service_factory)();
        let sim_train = Simulator::new(self.market_data.clone(), exec_train, config.clone());
        let result_train = sim_train
            .run_with_bars(
                symbol,
                &prefetched.train_bars,
                prefetched.train_start,
                prefetched.train_end,
                Some(prefetched.spy_train.clone()),
            )
            .await?;

        let exec_test = (self.execution_service_factory)();
        let sim_test = Simulator::new(self.market_data.clone(), exec_test, config.clone());
        let result_test = sim_test
            .run_with_bars(
                symbol,
                &prefetched.test_bars,
                prefetched.test_start,
                prefetched.test_end,
                Some(prefetched.spy_test.clone()),
            )
            .await?;

        let train_opt = Self::backtest_result_to_opt_result(config.clone(), result_train);
        let test_opt = Self::backtest_result_to_opt_result(config, result_test);
        Ok((train_opt, test_opt))
    }

    /// One evaluation with pre-fetched bars (for parallel run). Takes owned/Arc args so it can be spawned.
    async fn evaluate_one_with_bars(
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
        let train_opt = Self::backtest_result_to_opt_result(config.clone(), result_train);
        let test_opt = Self::backtest_result_to_opt_result(config, result_test);
        Ok((train_opt, test_opt))
    }

    /// One backtest over full period (single-period mode, for parallel run).
    async fn evaluate_one_single_period(
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
        Ok(Self::backtest_result_to_opt_result(config, result))
    }

    /// Run grid search optimization.
    /// - train_ratio >= 1.0: single period (one backtest on full range, fastest).
    /// - train_ratio in [0.5, 0.9]: walk-forward (train + test), rejects overfitting.
    pub async fn run_optimization(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        train_ratio: f64,
    ) -> Result<Vec<OptimizationResult>> {
        use chrono::Duration;

        const PARALLEL_WORKERS: usize = 4;
        let combinations = self.generate_combinations();
        let total_combinations = combinations.len();

        // Single period: one backtest per config on full range (no train/test split)
        if train_ratio >= 1.0 {
            info!("GridSearch: Single-period mode (train_ratio >= 1.0) — one backtest per config");
            info!("GridSearch: Pre-fetching bars for full period...");
            let bars = self
                .market_data
                .get_historical_bars(symbol, start, end, "1Min")
                .await
                .context("Failed to fetch bars")?;
            let spy_bars = self
                .market_data
                .get_historical_bars("SPY", start, end, "1Day")
                .await
                .unwrap_or_default();
            let prefetched = Arc::new(SinglePeriodBars {
                bars,
                start,
                end,
                spy_bars,
            });
            info!(
                "GridSearch: Loaded {} bars. Running {} combinations ({} workers)...",
                prefetched.bars.len(),
                total_combinations,
                PARALLEL_WORKERS
            );
            let progress_start = Instant::now();
            let market_data = self.market_data.clone();
            let execution_service_factory = self.execution_service_factory.clone();
            let symbol_owned = symbol.to_string();
            let completed: Vec<(usize, Result<OptimizationResult>)> =
                stream::iter(combinations.into_iter().enumerate())
                    .map(|(i, config)| {
                        let market_data = market_data.clone();
                        let execution_service_factory = execution_service_factory.clone();
                        let prefetched = Arc::clone(&prefetched);
                        let symbol_owned = symbol_owned.clone();
                        async move {
                            let r = Self::evaluate_one_single_period(
                                market_data,
                                execution_service_factory,
                                config,
                                symbol_owned,
                                prefetched,
                            )
                            .await;
                            (i, r)
                        }
                    })
                    .buffer_unordered(PARALLEL_WORKERS)
                    .collect()
                    .await;
            let elapsed_min = (progress_start.elapsed().as_secs_f64() / 60.0).round();
            info!(
                "GridSearch: Completed {} combinations in {} min",
                total_combinations, elapsed_min
            );
            let mut results: Vec<OptimizationResult> =
                completed.into_iter().filter_map(|(_, r)| r.ok()).collect();
            for r in &mut results {
                r.calculate_objective_score();
            }
            results.sort_by(|a, b| {
                b.sharpe_ratio
                    .partial_cmp(&a.sharpe_ratio)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(results);
        }

        // Walk-forward: train_ratio in [0.5, 0.9]
        let train_ratio = train_ratio.clamp(0.5, 0.9);
        let total_secs = (end - start).num_seconds();
        let train_secs = (total_secs as f64 * train_ratio) as i64;
        let train_end = start + Duration::seconds(train_secs);
        let test_start = train_end;

        if test_start >= end {
            anyhow::bail!(
                "Optimizer: period too short for train/test split (need test window after {:.0}% train)",
                train_ratio * 100.0
            );
        }

        debug!(
            "GridSearch: Walk-forward (train {:.0}% / test {:.0}%) {} combinations (train to {:?}, test to {:?})",
            train_ratio * 100.0,
            (1.0 - train_ratio) * 100.0,
            total_combinations,
            train_end,
            end
        );

        info!("GridSearch: Pre-fetching train/test and SPY bars...");
        let train_bars = self
            .market_data
            .get_historical_bars(symbol, start, train_end, "1Min")
            .await
            .context("Failed to fetch train bars")?;
        let test_bars = self
            .market_data
            .get_historical_bars(symbol, test_start, end, "1Min")
            .await
            .context("Failed to fetch test bars")?;
        let spy_train = self
            .market_data
            .get_historical_bars("SPY", start, train_end, "1Day")
            .await
            .unwrap_or_default();
        let spy_test = self
            .market_data
            .get_historical_bars("SPY", test_start, end, "1Day")
            .await
            .unwrap_or_default();
        let prefetched = Arc::new(PrefetchedBars {
            train_bars,
            train_start: start,
            train_end,
            test_bars,
            test_start,
            test_end: end,
            spy_train,
            spy_test,
        });
        info!(
            "GridSearch: Loaded {} train bars, {} test bars. Running {} combinations ({} workers)...",
            prefetched.train_bars.len(),
            prefetched.test_bars.len(),
            total_combinations,
            PARALLEL_WORKERS
        );

        // Estimate: ~3s per combo sequential; with 4 workers ~0.75 effective
        const SECS_PER_COMBO_ESTIMATE: f64 = 3.0;
        let estimated_total_min =
            (total_combinations as f64 * SECS_PER_COMBO_ESTIMATE / 60.0 / PARALLEL_WORKERS as f64)
                .ceil() as u64;
        debug!(
            "GridSearch: Estimated total time: ~{} min ({} workers)",
            estimated_total_min, PARALLEL_WORKERS
        );

        let progress_start = Instant::now();
        let market_data = self.market_data.clone();
        let execution_service_factory = self.execution_service_factory.clone();
        let symbol_owned = symbol.to_string();

        let completed: Vec<(usize, Result<(OptimizationResult, OptimizationResult)>)> =
            stream::iter(combinations.into_iter().enumerate())
                .map(|(i, config)| {
                    let market_data = market_data.clone();
                    let execution_service_factory = execution_service_factory.clone();
                    let prefetched = Arc::clone(&prefetched);
                    let symbol_owned = symbol_owned.clone();
                    async move {
                        let r = Self::evaluate_one_with_bars(
                            market_data,
                            execution_service_factory,
                            config,
                            symbol_owned,
                            prefetched,
                        )
                        .await;
                        (i, r)
                    }
                })
                .buffer_unordered(PARALLEL_WORKERS)
                .collect()
                .await;

        let elapsed_min = (progress_start.elapsed().as_secs_f64() / 60.0).round();
        info!(
            "GridSearch: Completed {} combinations in {} min",
            total_combinations, elapsed_min
        );

        let mut results = Vec::new();
        let mut by_index: Vec<_> = completed.into_iter().collect();
        by_index.sort_by_key(|(i, _)| *i);
        for (_i, train_opt) in by_index {
            match train_opt {
                Ok((train, mut test)) => {
                    test.in_sample_sharpe = Some(train.sharpe_ratio);
                    let sharpe_oos = test.sharpe_ratio;
                    let sharpe_is = train.sharpe_ratio;

                    if sharpe_is > Decimal::ZERO
                        && sharpe_oos
                            < Decimal::from_f64_retain(0.5).unwrap_or(Decimal::ZERO) * sharpe_is
                    {
                        debug!(
                            "GridSearch: Rejected (overfitting) - Sharpe OOS={:.2} < 0.5*IS={:.2}",
                            sharpe_oos, sharpe_is
                        );
                        continue;
                    }

                    test.calculate_objective_score();
                    debug!(
                        "GridSearch: Result - Sharpe IS={:.2} OOS={:.2}, Return={:.2}%, Score={:.4}",
                        sharpe_is, sharpe_oos, test.total_return, test.objective_score
                    );
                    results.push(test);
                }
                Err(e) => {
                    debug!("GridSearch: Evaluation failed: {}", e);
                }
            }
        }

        let total_elapsed_min = progress_start.elapsed().as_secs_f64() / 60.0;
        info!(
            "GridSearch: Completed {} combinations in {:.1} min ({} valid results)",
            total_combinations,
            total_elapsed_min,
            results.len()
        );

        // Sort by OOS Sharpe (descending)
        results.sort_by(|a, b| {
            b.sharpe_ratio
                .partial_cmp(&a.sharpe_ratio)
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

/// Genetic algorithm optimizer: evolves a population of parameter configs toward higher fitness.
pub struct GeneticOptimizer {
    market_data: Arc<dyn MarketDataService>,
    execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
    bounds: GeneBounds,
    strategy_mode: StrategyMode,
    min_profit_ratio: Decimal,
    population_size: usize,
    generations: usize,
    mutation_rate: f64,
    tournament_size: usize,
    /// When set, apply this risk appetite to each decoded config before evaluation.
    risk_score: Option<u8>,
}

impl GeneticOptimizer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        market_data: Arc<dyn MarketDataService>,
        execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
        bounds: GeneBounds,
        strategy_mode: StrategyMode,
        min_profit_ratio: Decimal,
        population_size: usize,
        generations: usize,
        mutation_rate: f64,
        risk_score: Option<u8>,
    ) -> Self {
        Self {
            market_data,
            execution_service_factory,
            bounds,
            strategy_mode,
            min_profit_ratio,
            population_size,
            generations,
            mutation_rate,
            tournament_size: 3,
            risk_score,
        }
    }

    /// Run genetic optimization (single period, parallel evaluation). Returns results sorted by objective.
    ///
    /// # Bottlenecks
    /// 1. **Bar count**: 1Min over years = 100k–400k+ bars. Each backtest iterates every bar (Analyst pipeline:
    ///    indicators, regime, signals). Use `timeframe = "5Min"` or `"15Min"` to reduce bars (≈5× or 15× faster).
    /// 2. **API fetch**: Alpaca returns 10k bars per page; large ranges = many round trips. Shorten date range
    ///    or use coarser timeframe to reduce fetch time.
    /// 3. **Per-bar work**: For each bar we call get_portfolio, update_indicators, regime detection, signal
    ///    generation. Cost is O(bars × backtests); reducing bars (timeframe) has the largest impact.
    pub async fn run_optimization(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
    ) -> Result<Vec<OptimizationResult>> {
        const PARALLEL_WORKERS: usize = 4;
        let timeframe = if timeframe.is_empty() {
            "1Min"
        } else {
            timeframe
        };
        info!(
            "GeneticOptimizer: Pre-fetching bars (timeframe={})...",
            timeframe
        );
        eprintln!(
            "[optimize] Fetching {} bars for {} ({} to {}) — may take several minutes for large ranges...",
            timeframe,
            symbol,
            start.format("%Y-%m-%d"),
            end.format("%Y-%m-%d")
        );
        let bars = self
            .market_data
            .get_historical_bars(symbol, start, end, timeframe)
            .await
            .context("Failed to fetch bars")?;
        eprintln!(
            "[optimize] Loaded {} bars. Fetching SPY benchmark...",
            bars.len()
        );
        let spy_bars = self
            .market_data
            .get_historical_bars("SPY", start, end, "1Day")
            .await
            .unwrap_or_default();
        eprintln!("[optimize] Data ready. Starting evolution...");
        let bar_count = bars.len();
        let prefetched = Arc::new(SinglePeriodBars {
            bars,
            start,
            end,
            spy_bars,
        });
        info!(
            "GeneticOptimizer: Loaded {} bars. Running {} individuals × {} generations ({} workers)",
            bar_count, self.population_size, self.generations, PARALLEL_WORKERS
        );
        let progress_start = Instant::now();

        const GENOME_LEN: usize = 14;
        let mut population: Vec<[f64; GENOME_LEN]> = (0..self.population_size)
            .map(|_| {
                [
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                    rand::random::<f64>(),
                ]
            })
            .collect();

        let market_data = self.market_data.clone();
        let execution_service_factory = self.execution_service_factory.clone();
        let bounds = self.bounds.clone();
        let strategy_mode = self.strategy_mode;
        let min_profit_ratio = self.min_profit_ratio;
        let symbol_owned = symbol.to_string();
        let mut global_best: Option<(OptimizationResult, [f64; GENOME_LEN])> = None;

        for generation in 0..self.generations {
            let elapsed_secs = progress_start.elapsed().as_secs();
            let elapsed_min = elapsed_secs / 60;
            eprintln!(
                "[optimize] Generation {}/{} — evaluating {} individuals... ({} min elapsed)",
                generation + 1,
                self.generations,
                self.population_size,
                elapsed_min
            );
            info!(
                "GeneticOptimizer: Generation {}/{} — evaluating {} individuals... ({} min elapsed)",
                generation + 1,
                self.generations,
                self.population_size,
                elapsed_min
            );

            let risk_score = self.risk_score;
            let configs: Vec<AnalystConfig> = population
                .iter()
                .map(|g| {
                    let mut config = decode_genome(g, &bounds, strategy_mode, min_profit_ratio);
                    if let Some(score) = risk_score
                        && let Ok(ra) = RiskAppetite::new(score)
                    {
                        config.apply_risk_appetite(&ra);
                    }
                    config
                })
                .collect();

            // Keep (population_index, result) so elitism preserves the correct genome (buffer_unordered reorders completion)
            let completed: Vec<(usize, Result<OptimizationResult>)> =
                stream::iter(configs.into_iter().enumerate())
                    .map(|(i, config)| {
                        let market_data = market_data.clone();
                        let execution_service_factory = execution_service_factory.clone();
                        let prefetched = Arc::clone(&prefetched);
                        let symbol_owned = symbol_owned.clone();
                        async move {
                            let r = run_single_period_eval(
                                market_data,
                                execution_service_factory,
                                config,
                                symbol_owned,
                                prefetched,
                            )
                            .await;
                            (i, r)
                        }
                    })
                    .buffer_unordered(PARALLEL_WORKERS)
                    .collect()
                    .await;

            let mut scored: Vec<(usize, OptimizationResult)> = completed
                .into_iter()
                .filter_map(|(i, r)| r.ok().map(|res| (i, res)))
                .collect();
            for (_, res) in &mut scored {
                res.calculate_objective_score();
            }
            scored.sort_by(|a, b| {
                b.1.objective_score
                    .partial_cmp(&a.1.objective_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // Track global best so we never lose the best solution across generations
            if let Some((best_idx, best_res)) = scored.first() {
                let best_so_far = best_res.objective_score.to_f64().unwrap_or(-1e9);
                let is_better = global_best
                    .as_ref()
                    .is_none_or(|(g, _)| best_so_far > g.objective_score.to_f64().unwrap_or(-1e9));
                if is_better {
                    global_best = Some((best_res.clone(), population[*best_idx]));
                }
            }

            let fitness: Vec<f64> = scored
                .iter()
                .map(|(_, r)| r.objective_score.to_f64().unwrap_or(-1e9))
                .collect();
            let best_fitness = fitness.first().copied().unwrap_or(-1e9);
            let gen_elapsed_secs = progress_start.elapsed().as_secs();
            let gen_elapsed_min = gen_elapsed_secs / 60;
            let valid_count = scored.len();
            eprintln!(
                "[optimize] Gen {}/{} done — best score = {:.4} ({} valid, {} min total)",
                generation + 1,
                self.generations,
                best_fitness,
                valid_count,
                gen_elapsed_min
            );
            info!(
                "GeneticOptimizer: gen {}/{} done — best score = {:.4} ({} valid, {} min total)",
                generation + 1,
                self.generations,
                best_fitness,
                valid_count,
                gen_elapsed_min
            );

            if generation + 1 == self.generations {
                let mut results: Vec<OptimizationResult> =
                    scored.into_iter().map(|(_, r)| r).collect();
                for r in &mut results {
                    r.risk_score = self.risk_score;
                }
                results.sort_by(|a, b| {
                    b.objective_score
                        .partial_cmp(&a.objective_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                // Ensure global best across all generations is in results and first
                if let Some((ref mut gb_res, _)) = global_best {
                    gb_res.risk_score = self.risk_score;
                    let gb_score = gb_res.objective_score.to_f64().unwrap_or(-1e9);
                    let current_best = results
                        .first()
                        .and_then(|r| r.objective_score.to_f64())
                        .unwrap_or(-1e9);
                    if gb_score > current_best {
                        results.insert(0, gb_res.clone());
                    }
                }
                let total_min = progress_start.elapsed().as_secs() / 60;
                let top_score = results
                    .first()
                    .and_then(|r| r.objective_score.to_f64())
                    .unwrap_or(0.0);
                info!(
                    "GeneticOptimizer: Completed in {} min. Top score = {:.4}, {} results",
                    total_min,
                    top_score,
                    results.len()
                );
                return Ok(results);
            }

            let indices_by_fitness: Vec<usize> = scored.into_iter().map(|(i, _)| i).collect();
            let mut new_population = Vec::with_capacity(self.population_size);
            new_population.push(population[indices_by_fitness[0]]);
            while new_population.len() < self.population_size {
                let p1 = self.tournament_select(&population, &indices_by_fitness);
                let p2 = self.tournament_select(&population, &indices_by_fitness);
                let mut child = self.crossover(&p1, &p2);
                self.mutate(&mut child);
                new_population.push(child);
            }
            population = new_population;
        }

        Ok(Vec::new())
    }

    /// Tournament selection: pick k random by rank, return genome of best (lowest rank index = best).
    fn tournament_select(
        &self,
        population: &[[f64; 14]],
        indices_by_fitness: &[usize],
    ) -> [f64; 14] {
        let n = indices_by_fitness.len();
        if n == 0 {
            return [0.5; 14];
        }
        let k = self.tournament_size.min(n);
        let mut best_rank = rand::random_range(0..n);
        for _ in 1..k {
            let r = rand::random_range(0..n);
            if r < best_rank {
                best_rank = r;
            }
        }
        population[indices_by_fitness[best_rank]]
    }

    fn crossover(&self, p1: &[f64; 14], p2: &[f64; 14]) -> [f64; 14] {
        let mut child = [0.0_f64; 14];
        for i in 0..14 {
            child[i] = if rand::random::<bool>() { p1[i] } else { p2[i] };
        }
        child
    }

    fn mutate(&self, genome: &mut [f64; 14]) {
        for g in genome.iter_mut() {
            if rand::random::<f64>() < self.mutation_rate {
                *g = (*g + rand::random_range(-0.2_f64..=0.2)).clamp(0.0, 1.0);
            }
        }
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
            ..Default::default()
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
                ensemble_voting_threshold: dec!(0.5),
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
                stat_momentum_lookback: 10,
                stat_momentum_threshold: dec!(1.5),
                stat_momentum_trend_confirmation: true,
                zscore_lookback: 20,
                zscore_entry_threshold: dec!(-2.0),
                zscore_exit_threshold: dec!(0.0),
                orderflow_ofi_threshold: dec!(0.3),
                orderflow_stacked_count: 3,
                orderflow_volume_profile_lookback: 100,
                ensemble_weights: Default::default(),
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

        // Score = (2.0 * 0.4) + (0.15 * 0.3) + (0.6 * 0.2) - (0.05 * 0.1)
        //       = 0.8 + 0.045 + 0.12 - 0.005 = 0.96
        assert!((result.objective_score - dec!(0.96)).abs() < dec!(0.01));
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
        assert_eq!(first.stat_momentum_lookback, 10);
        assert_eq!(first.zscore_lookback, 20);
        assert_eq!(first.orderflow_stacked_count, 3);
    }
}
