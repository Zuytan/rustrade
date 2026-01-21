use crate::domain::trading::fee_model::{ConstantFeeModel, FeeModel};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn default_fee_model() -> Arc<dyn FeeModel> {
    Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystConfig {
    pub fast_sma_period: usize,
    pub slow_sma_period: usize,
    pub max_positions: usize,
    pub trade_quantity: Decimal,
    pub sma_threshold: f64,
    pub order_cooldown_seconds: u64,
    pub risk_per_trade_percent: f64,
    pub strategy_mode: crate::domain::market::strategy_config::StrategyMode,
    pub trend_sma_period: usize,
    pub rsi_period: usize,
    pub macd_fast_period: usize,
    pub macd_slow_period: usize,
    pub macd_signal_period: usize,
    pub trend_divergence_threshold: f64,
    pub trailing_stop_atr_multiplier: f64,
    pub atr_period: usize,
    pub rsi_threshold: f64,                // New Configurable Threshold
    pub trend_riding_exit_buffer_pct: f64, // Trend Riding Strategy
    pub mean_reversion_rsi_exit: f64,
    pub mean_reversion_bb_period: usize,
    #[serde(skip, default = "default_fee_model")] // FeeModel is trait object
    pub fee_model: Arc<dyn FeeModel>,
    pub max_position_size_pct: f64,
    pub bb_std_dev: f64,
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,
    pub take_profit_pct: f64,
    pub min_hold_time_minutes: i64,      // Phase 2: minimum hold time
    pub signal_confirmation_bars: usize, // Phase 2: signal confirmation
    pub spread_bps: f64,                 // Cost-aware trading: spread in basis points
    pub min_profit_ratio: f64,           // Cost-aware trading: minimum profit/cost ratio
    pub profit_target_multiplier: f64,
    // Risk-based adaptive filters
    pub macd_requires_rising: bool, // Whether MACD must be rising for buy signals
    pub trend_tolerance_pct: f64,   // Percentage tolerance for trend filter
    pub macd_min_threshold: f64,    // Minimum MACD histogram threshold
    pub adx_period: usize,
    pub adx_threshold: f64,
    // SMC Strategy Configuration
    pub smc_ob_lookback: usize,          // Order Block lookback period
    pub smc_min_fvg_size_pct: f64,       // Minimum Fair Value Gap size (e.g., 0.005 = 0.5%)
    pub smc_volume_multiplier: f64, // Volume multiplier for OB confirmation (e.g. 1.5x average)
    pub risk_appetite_score: Option<u8>, // Base Risk Appetite Score (1-9) for dynamic scaling
    // Breakout Strategy Configuration
    pub breakout_lookback: usize,
    pub breakout_threshold_pct: f64,
    pub breakout_volume_mult: f64,
    // Hard Stop Configuration
    pub max_loss_per_trade_pct: f64, // Maximum loss per trade before forced exit (e.g., -0.05 = -5%)
}

impl Default for AnalystConfig {
    fn default() -> Self {
        Self {
            fast_sma_period: 10,
            slow_sma_period: 20,
            max_positions: 5,
            trade_quantity: rust_decimal::Decimal::ONE,
            sma_threshold: 0.005, // Raised from 0.001 - after signal sensitivity, Risk-2 gets ~0.0025 (0.25%)
            order_cooldown_seconds: 60,
            risk_per_trade_percent: 1.0,
            strategy_mode: Default::default(),
            trend_sma_period: 50,
            rsi_period: 14,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            trend_divergence_threshold: 0.05,
            trailing_stop_atr_multiplier: 2.0,
            atr_period: 14,
            rsi_threshold: 70.0,
            trend_riding_exit_buffer_pct: 0.02,
            mean_reversion_rsi_exit: 50.0,
            mean_reversion_bb_period: 20,
            fee_model: Arc::new(ConstantFeeModel::new(
                rust_decimal::Decimal::ZERO,
                rust_decimal::Decimal::ZERO,
            )),
            max_position_size_pct: 10.0,
            bb_std_dev: 2.0,
            ema_fast_period: 10,
            ema_slow_period: 20,
            take_profit_pct: 0.1,
            min_hold_time_minutes: 0,
            signal_confirmation_bars: 1,
            spread_bps: 0.0,
            min_profit_ratio: 1.5,
            profit_target_multiplier: 2.0,
            macd_requires_rising: false,
            trend_tolerance_pct: 0.02,
            macd_min_threshold: 0.0,
            adx_period: 14,
            adx_threshold: 25.0,
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: 0.005,
            smc_volume_multiplier: 1.5,
            risk_appetite_score: None,
            breakout_lookback: 10,
            breakout_threshold_pct: 0.002,
            breakout_volume_mult: 1.1,
            max_loss_per_trade_pct: -0.05, // -5% max loss per trade
        }
    }
}

impl From<crate::config::Config> for AnalystConfig {
    fn from(config: crate::config::Config) -> Self {
        Self {
            fast_sma_period: config.fast_sma_period,
            slow_sma_period: config.slow_sma_period,
            max_positions: config.max_positions,
            trade_quantity: config.trade_quantity,
            sma_threshold: config.sma_threshold,
            order_cooldown_seconds: config.order_cooldown_seconds,
            risk_per_trade_percent: config.risk_per_trade_percent,
            strategy_mode: config.strategy_mode,
            trend_sma_period: config.trend_sma_period,
            rsi_period: config.rsi_period,
            macd_fast_period: config.macd_fast_period,
            macd_slow_period: config.macd_slow_period,
            macd_signal_period: config.macd_signal_period,
            trend_divergence_threshold: config.trend_divergence_threshold,
            rsi_threshold: config.rsi_threshold,
            trailing_stop_atr_multiplier: config.trailing_stop_atr_multiplier,
            atr_period: config.atr_period,
            trend_riding_exit_buffer_pct: config.trend_riding_exit_buffer_pct,
            mean_reversion_rsi_exit: config.mean_reversion_rsi_exit,
            mean_reversion_bb_period: config.mean_reversion_bb_period,
            fee_model: config.create_fee_model(),
            max_position_size_pct: config.max_position_size_pct,
            bb_std_dev: 2.0,
            ema_fast_period: config.ema_fast_period,
            ema_slow_period: config.ema_slow_period,
            take_profit_pct: config.take_profit_pct,
            min_hold_time_minutes: config.min_hold_time_minutes,
            signal_confirmation_bars: config.signal_confirmation_bars,
            spread_bps: config.spread_bps,
            min_profit_ratio: config.min_profit_ratio,
            profit_target_multiplier: config.profit_target_multiplier,
            macd_requires_rising: config.macd_requires_rising,
            trend_tolerance_pct: config.trend_tolerance_pct,
            macd_min_threshold: config.macd_min_threshold,
            adx_period: config.adx_period,
            adx_threshold: config.adx_threshold,
            smc_ob_lookback: config.smc_ob_lookback,
            smc_min_fvg_size_pct: config.smc_min_fvg_size_pct,
            smc_volume_multiplier: 1.5, // Default, not yet in base Config
            risk_appetite_score: config.risk_appetite.map(|r| r.score()),
            breakout_lookback: 20, // Increased lookback for more significant levels
            breakout_threshold_pct: 0.0005, // 0.05% threshold (sensitive)
            breakout_volume_mult: 0.1, // 10% of average (effectively disable volume filter for now)
            max_loss_per_trade_pct: -0.05,
        }
    }
}

impl AnalystConfig {
    pub fn apply_risk_appetite(
        &mut self,
        appetite: &crate::domain::risk::risk_appetite::RiskAppetite,
    ) {
        self.risk_per_trade_percent = appetite.calculate_risk_per_trade_percent();
        self.trailing_stop_atr_multiplier = appetite.calculate_trailing_stop_multiplier();
        self.rsi_threshold = appetite.calculate_rsi_threshold();
        self.max_position_size_pct = appetite.calculate_max_position_size_pct();
        self.min_profit_ratio = appetite.calculate_min_profit_ratio();
        self.macd_requires_rising = appetite.requires_macd_rising();
        self.trend_tolerance_pct = appetite.calculate_trend_tolerance_pct();
        self.macd_min_threshold = appetite.calculate_macd_min_threshold();
        self.profit_target_multiplier = appetite.calculate_profit_target_multiplier();

        // Apply signal sensitivity factor for lower risk profiles
        // This makes Conservative/Balanced profiles generate more signals
        let sensitivity = appetite.calculate_signal_sensitivity_factor();
        self.sma_threshold *= sensitivity;

        // Reduce confirmation bars for conservative profiles (1 for score <= 4, else keep)
        if appetite.score() <= 4 {
            self.signal_confirmation_bars = 1;
        }
    }
}

impl From<&AnalystConfig> for crate::application::risk_management::sizing_engine::SizingConfig {
    fn from(config: &AnalystConfig) -> Self {
        Self {
            risk_per_trade_percent: config.risk_per_trade_percent,
            max_positions: config.max_positions,
            max_position_size_pct: config.max_position_size_pct,
            static_trade_quantity: config.trade_quantity,
        }
    }
}
