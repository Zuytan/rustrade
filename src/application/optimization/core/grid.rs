use crate::application::agents::analyst_config::AnalystConfig;
use crate::config::StrategyMode;
use crate::domain::config::StrategyConfig;
use crate::domain::risk::risk_config::RiskConfig;
use crate::domain::trading::fee_model::ConstantFeeModel;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

pub fn decode_genome(
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
        strategy: StrategyConfig {
            strategy_mode,
            fast_sma_period: fast,
            slow_sma_period: slow,
            trend_sma_period: 2000,
            sma_threshold: dec!(0.001),
            rsi_period: 14,
            rsi_threshold: rsi,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            macd_requires_rising: true,
            macd_min_threshold: dec!(0.0),
            ema_fast_period: 50,
            ema_slow_period: 150,
            adx_period: 14,
            adx_threshold: dec!(25.0),
            regime_volatility_threshold: dec!(2.0),
            bb_std_dev: dec!(2.0),
            spread_bps: dec!(5.0),
            atr_period: 14,
            trailing_stop_atr_multiplier: atr_mult,
            take_profit_pct: dec!(0.05),
            profit_target_multiplier: dec!(1.5),
            trend_divergence_threshold: trend_div,
            trend_tolerance_pct: dec!(0.0),
            trend_riding_exit_buffer_pct: dec!(0.03),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            signal_confirmation_bars: 1,
            smc_ob_lookback,
            smc_min_fvg_size_pct: smc_min_fvg,
            smc_volume_multiplier: dec!(1.5),
            breakout_lookback: 10,
            breakout_threshold_pct: dec!(0.002),
            breakout_volume_mult: dec!(1.1),
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
            primary_timeframe: crate::domain::market::timeframe::Timeframe::OneMin,
            enabled_timeframes: vec![crate::domain::market::timeframe::Timeframe::OneMin],
            trend_timeframe: crate::domain::market::timeframe::Timeframe::OneHour,
            enable_ml_data_collection: false,
            risk_appetite_score: None,
            min_profit_ratio,
            ..StrategyConfig::default()
        },
        risk: RiskConfig {
            max_positions: 5,
            risk_per_trade_percent: dec!(0.02),
            max_position_size_pct: dec!(0.1),
            max_loss_per_trade_pct: dec!(-0.05),
            trade_quantity: Decimal::from(1),
            order_cooldown_seconds: cooldown,
            min_hold_time_minutes: 0,
            ..Default::default()
        },
        fee_model: Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
    }
}
