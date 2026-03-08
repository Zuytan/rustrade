//! Strategy Configuration Domain Value Object
//!
//! This module defines the `StrategyConfig` value object, which encapsulates
//! all strategy-related parameters.

use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::market::timeframe::Timeframe;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error type for StrategyConfig validation
#[derive(Debug, Error, PartialEq)]
pub enum StrategyConfigError {
    #[error("Invalid period: {field} = {value}. Must be > 0")]
    InvalidPeriod { field: String, value: usize },

    #[error("Invalid threshold: {field} = {value}. Must be positive")]
    InvalidThreshold { field: String, value: Decimal },

    #[error("Empty timeframes list")]
    EmptyTimeframes,
}

/// Strategy configuration value object
///
/// # Invariants
///
/// - All period fields must be > 0
/// - All threshold fields must be >= 0.0
/// - `enabled_timeframes` must not be empty
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub strategy_mode: StrategyMode,

    // SMA Configuration
    pub fast_sma_period: usize,
    pub slow_sma_period: usize,
    pub trend_sma_period: usize,
    pub sma_threshold: Decimal,

    // RSI Configuration
    pub rsi_period: usize,
    pub rsi_threshold: Decimal,

    // MACD Configuration
    pub macd_fast_period: usize,
    pub macd_slow_period: usize,
    pub macd_signal_period: usize,
    pub macd_requires_rising: bool,
    pub macd_min_threshold: Decimal,

    // EMA Configuration
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,

    // ADX Configuration
    pub adx_period: usize,
    pub adx_threshold: Decimal,
    pub regime_volatility_threshold: Decimal,

    // BB Configuration
    pub bb_std_dev: Decimal,

    // ATR & Stops
    pub atr_period: usize,
    pub trailing_stop_atr_multiplier: Decimal,
    pub take_profit_pct: Decimal,
    pub min_profit_ratio: Decimal,
    pub profit_target_multiplier: Decimal,

    // Trend Configuration
    pub trend_divergence_threshold: Decimal,
    pub trend_tolerance_pct: Decimal,
    pub trend_riding_exit_buffer_pct: Decimal,

    // Mean Reversion
    pub mean_reversion_rsi_exit: Decimal,
    pub mean_reversion_bb_period: usize,

    // Signal Confirmation
    pub signal_confirmation_bars: usize,

    // SMC (Smart Money Concepts)
    pub smc_ob_lookback: usize,
    pub smc_min_fvg_size_pct: Decimal,
    pub smc_volume_multiplier: Decimal,

    // Breakout Configuration
    pub breakout_lookback: usize,
    pub breakout_threshold_pct: Decimal,
    pub breakout_volume_mult: Decimal,

    // Statistical Momentum
    pub stat_momentum_lookback: usize,
    pub stat_momentum_threshold: Decimal,
    pub stat_momentum_trend_confirmation: bool,

    // Z-Score Mean Reversion
    pub zscore_lookback: usize,
    pub zscore_entry_threshold: Decimal,
    pub zscore_exit_threshold: Decimal,

    // Order Flow
    pub orderflow_ofi_threshold: Decimal,
    pub orderflow_stacked_count: usize,
    pub orderflow_volume_profile_lookback: usize,

    // Ensemble Configuration
    pub ensemble_weights: Option<std::collections::HashMap<String, f64>>,
    pub ensemble_voting_threshold: Decimal,

    // Multi-Timeframe
    pub primary_timeframe: Timeframe,
    pub enabled_timeframes: Vec<Timeframe>,
    pub trend_timeframe: Timeframe,

    // ML Configuration
    pub enable_ml_data_collection: bool,

    // Risk Scaling
    pub risk_appetite_score: Option<u8>,

    // Market / Execution specific
    pub spread_bps: Decimal,
}

impl StrategyConfig {
    /// Create a new StrategyConfig with validation.
    /// Note: This replaces the previous multi-argument new with a simpler construction.
    /// Invariants are maintained by calling validate() after initialization.
    pub fn validate(&self) -> Result<(), StrategyConfigError> {
        // Validate periods
        self.validate_period("fast_sma_period", self.fast_sma_period)?;
        self.validate_period("slow_sma_period", self.slow_sma_period)?;
        self.validate_period("trend_sma_period", self.trend_sma_period)?;
        self.validate_period("rsi_period", self.rsi_period)?;
        self.validate_period("macd_fast_period", self.macd_fast_period)?;
        self.validate_period("macd_slow_period", self.macd_slow_period)?;
        self.validate_period("macd_signal_period", self.macd_signal_period)?;
        self.validate_period("adx_period", self.adx_period)?;
        self.validate_period("signal_confirmation_bars", self.signal_confirmation_bars)?;
        self.validate_period("atr_period", self.atr_period)?;

        // Validate thresholds
        self.validate_threshold("rsi_threshold", self.rsi_threshold)?;
        self.validate_threshold("adx_threshold", self.adx_threshold)?;
        self.validate_threshold(
            "trend_divergence_threshold",
            self.trend_divergence_threshold,
        )?;
        self.validate_threshold("sma_threshold", self.sma_threshold)?;
        self.validate_threshold("bb_std_dev", self.bb_std_dev)?;
        self.validate_threshold(
            "regime_volatility_threshold",
            self.regime_volatility_threshold,
        )?;

        // Validate timeframes
        if self.enabled_timeframes.is_empty() {
            return Err(StrategyConfigError::EmptyTimeframes);
        }

        Ok(())
    }

    fn validate_period(&self, field: &str, value: usize) -> Result<(), StrategyConfigError> {
        if value == 0 {
            return Err(StrategyConfigError::InvalidPeriod {
                field: field.to_string(),
                value,
            });
        }
        Ok(())
    }

    fn validate_threshold(&self, field: &str, value: Decimal) -> Result<(), StrategyConfigError> {
        if value < Decimal::ZERO {
            return Err(StrategyConfigError::InvalidThreshold {
                field: field.to_string(),
                value,
            });
        }
        Ok(())
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            strategy_mode: StrategyMode::Dynamic,
            fast_sma_period: 20,
            slow_sma_period: 60,
            trend_sma_period: 50,
            sma_threshold: dec!(0.005),
            rsi_period: 14,
            rsi_threshold: dec!(70.0),
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            macd_requires_rising: false,
            macd_min_threshold: Decimal::ZERO,
            ema_fast_period: 10,
            ema_slow_period: 20,
            adx_period: 14,
            adx_threshold: dec!(25.0),
            regime_volatility_threshold: dec!(2.0),
            bb_std_dev: dec!(2.0),
            atr_period: 14,
            trailing_stop_atr_multiplier: dec!(2.0),
            take_profit_pct: dec!(0.1),
            min_profit_ratio: dec!(1.5),
            profit_target_multiplier: dec!(2.0),
            trend_divergence_threshold: dec!(0.05),
            trend_tolerance_pct: dec!(0.02),
            trend_riding_exit_buffer_pct: dec!(0.02),
            mean_reversion_rsi_exit: dec!(50.0),
            mean_reversion_bb_period: 20,
            signal_confirmation_bars: 1,
            smc_ob_lookback: 20,
            smc_min_fvg_size_pct: dec!(0.005),
            smc_volume_multiplier: dec!(1.5),
            breakout_lookback: 10,
            breakout_threshold_pct: dec!(0.002),
            breakout_volume_mult: dec!(1.1),
            stat_momentum_lookback: 10,
            stat_momentum_threshold: dec!(0.8),
            stat_momentum_trend_confirmation: true,
            zscore_lookback: 20,
            zscore_entry_threshold: dec!(-1.5),
            zscore_exit_threshold: dec!(0.0),
            orderflow_ofi_threshold: dec!(0.3),
            orderflow_stacked_count: 3,
            orderflow_volume_profile_lookback: 100,
            ensemble_weights: None,
            ensemble_voting_threshold: dec!(0.5),
            primary_timeframe: Timeframe::OneMin,
            enabled_timeframes: vec![
                Timeframe::OneMin,
                Timeframe::FiveMin,
                Timeframe::FifteenMin,
                Timeframe::OneHour,
            ],
            trend_timeframe: Timeframe::OneHour,
            enable_ml_data_collection: true,
            risk_appetite_score: None,
            spread_bps: dec!(0.5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let config = StrategyConfig {
            fast_sma_period: 20,
            slow_sma_period: 60,
            ..StrategyConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_period() {
        let config = StrategyConfig {
            fast_sma_period: 0,
            ..StrategyConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_empty_timeframes() {
        let config = StrategyConfig {
            enabled_timeframes: vec![],
            ..StrategyConfig::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), StrategyConfigError::EmptyTimeframes);
    }

    #[test]
    fn test_default_config() {
        let config = StrategyConfig::default();
        assert_eq!(config.fast_sma_period, 20);
        assert_eq!(config.slow_sma_period, 60);
        assert!(!config.enabled_timeframes.is_empty());
    }
}
