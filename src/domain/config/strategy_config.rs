//! Strategy Configuration Domain Value Object
//!
//! This module defines the `StrategyConfig` value object, which encapsulates
//! all strategy-related parameters.

use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::market::timeframe::Timeframe;
use thiserror::Error;

/// Error type for StrategyConfig validation
#[derive(Debug, Error, PartialEq)]
pub enum StrategyConfigError {
    #[error("Invalid period: {field} = {value}. Must be > 0")]
    InvalidPeriod { field: String, value: usize },

    #[error("Invalid threshold: {field} = {value}. Must be positive")]
    InvalidThreshold { field: String, value: f64 },

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
#[derive(Debug, Clone, PartialEq)]
pub struct StrategyConfig {
    pub strategy_mode: StrategyMode,

    // SMA Configuration
    pub fast_sma_period: usize,
    pub slow_sma_period: usize,
    pub trend_sma_period: usize,

    // RSI Configuration
    pub rsi_period: usize,
    pub rsi_threshold: f64,

    // MACD Configuration
    pub macd_fast_period: usize,
    pub macd_slow_period: usize,
    pub macd_signal_period: usize,
    pub macd_requires_rising: bool,
    pub macd_min_threshold: f64,

    // ADX Configuration
    pub adx_period: usize,
    pub adx_threshold: f64,

    // Trend Configuration
    pub trend_divergence_threshold: f64,
    pub trend_tolerance_pct: f64,

    // Signal Confirmation
    pub signal_confirmation_bars: usize,

    // Multi-Timeframe
    pub primary_timeframe: Timeframe,
    pub enabled_timeframes: Vec<Timeframe>,
    pub trend_timeframe: Timeframe,
}

impl StrategyConfig {
    /// Create a new StrategyConfig with validation
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        strategy_mode: StrategyMode,
        fast_sma_period: usize,
        slow_sma_period: usize,
        trend_sma_period: usize,
        rsi_period: usize,
        rsi_threshold: f64,
        macd_fast_period: usize,
        macd_slow_period: usize,
        macd_signal_period: usize,
        macd_requires_rising: bool,
        macd_min_threshold: f64,
        adx_period: usize,
        adx_threshold: f64,
        trend_divergence_threshold: f64,
        trend_tolerance_pct: f64,
        signal_confirmation_bars: usize,
        primary_timeframe: Timeframe,
        enabled_timeframes: Vec<Timeframe>,
        trend_timeframe: Timeframe,
    ) -> Result<Self, StrategyConfigError> {
        let config = Self {
            strategy_mode,
            fast_sma_period,
            slow_sma_period,
            trend_sma_period,
            rsi_period,
            rsi_threshold,
            macd_fast_period,
            macd_slow_period,
            macd_signal_period,
            macd_requires_rising,
            macd_min_threshold,
            adx_period,
            adx_threshold,
            trend_divergence_threshold,
            trend_tolerance_pct,
            signal_confirmation_bars,
            primary_timeframe,
            enabled_timeframes,
            trend_timeframe,
        };

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), StrategyConfigError> {
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

        // Validate thresholds
        self.validate_threshold("rsi_threshold", self.rsi_threshold)?;
        self.validate_threshold("adx_threshold", self.adx_threshold)?;
        self.validate_threshold(
            "trend_divergence_threshold",
            self.trend_divergence_threshold,
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

    fn validate_threshold(&self, field: &str, value: f64) -> Result<(), StrategyConfigError> {
        if value < 0.0 {
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
        Self {
            strategy_mode: StrategyMode::Dynamic,
            fast_sma_period: 20,
            slow_sma_period: 60,
            trend_sma_period: 50,
            rsi_period: 14,
            rsi_threshold: 75.0,
            macd_fast_period: 12,
            macd_slow_period: 26,
            macd_signal_period: 9,
            macd_requires_rising: true,
            macd_min_threshold: 0.0,
            adx_period: 14,
            adx_threshold: 25.0,
            trend_divergence_threshold: 0.005,
            trend_tolerance_pct: 0.0,
            signal_confirmation_bars: 2,
            primary_timeframe: Timeframe::OneMin,
            enabled_timeframes: vec![
                Timeframe::OneMin,
                Timeframe::FiveMin,
                Timeframe::FifteenMin,
                Timeframe::OneHour,
            ],
            trend_timeframe: Timeframe::OneHour,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let config = StrategyConfig::new(
            StrategyMode::Dynamic,
            20,
            60,
            50,
            14,
            75.0,
            12,
            26,
            9,
            true,
            0.0,
            14,
            25.0,
            0.005,
            0.0,
            2,
            Timeframe::OneMin,
            vec![Timeframe::OneMin],
            Timeframe::OneHour,
        );
        assert!(config.is_ok());
    }

    #[test]
    fn test_invalid_period() {
        let result = StrategyConfig::new(
            StrategyMode::Dynamic,
            0,
            60,
            50,
            14,
            75.0, // fast_sma_period = 0
            12,
            26,
            9,
            true,
            0.0,
            14,
            25.0,
            0.005,
            0.0,
            2,
            Timeframe::OneMin,
            vec![Timeframe::OneMin],
            Timeframe::OneHour,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_timeframes() {
        let result = StrategyConfig::new(
            StrategyMode::Dynamic,
            20,
            60,
            50,
            14,
            75.0,
            12,
            26,
            9,
            true,
            0.0,
            14,
            25.0,
            0.005,
            0.0,
            2,
            Timeframe::OneMin,
            vec![], // Empty
            Timeframe::OneHour,
        );
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
