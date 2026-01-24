//! Strategy configuration parsing from environment variables.
//!
//! This module handles loading technical indicator and strategy parameters.

use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::market::timeframe::Timeframe;
use crate::domain::risk::risk_appetite::RiskAppetite;
use anyhow::{Context, Result};
use std::env;
use std::str::FromStr;

/// Strategy environment configuration
#[derive(Debug, Clone)]
pub struct StrategyEnvConfig {
    // Core SMA
    pub fast_sma_period: usize,
    pub slow_sma_period: usize,
    pub trend_sma_period: usize,
    pub sma_threshold: f64,

    // RSI
    pub rsi_period: usize,
    pub rsi_threshold: f64,

    // MACD
    pub macd_fast_period: usize,
    pub macd_slow_period: usize,
    pub macd_signal_period: usize,
    pub macd_requires_rising: bool,
    pub macd_min_threshold: f64,

    // EMA
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,

    // ADX
    pub adx_period: usize,
    pub adx_threshold: f64,

    // ATR
    pub atr_period: usize,
    pub trailing_stop_atr_multiplier: f64,

    // Strategy mode
    pub strategy_mode: StrategyMode,
    pub trend_divergence_threshold: f64,
    pub trend_tolerance_pct: f64,

    // Mean Reversion
    pub mean_reversion_rsi_exit: f64,
    pub mean_reversion_bb_period: usize,
    pub trend_riding_exit_buffer_pct: f64,

    // SMC (Smart Money Concepts)
    pub smc_ob_lookback: usize,
    pub smc_min_fvg_size_pct: f64,

    // Timeframes
    pub primary_timeframe: Timeframe,
    pub enabled_timeframes: Vec<Timeframe>,
    pub trend_timeframe: Timeframe,

    // Signal Parameters
    pub signal_confirmation_bars: usize,
    pub take_profit_pct: f64,
    pub profit_target_multiplier: f64,

    // Risk Appetite Override
    pub risk_appetite: Option<RiskAppetite>,

    // ML Configuration
    pub enable_ml_data_collection: bool,
}

impl StrategyEnvConfig {
    pub fn from_env() -> Result<Self> {
        let strategy_mode_str =
            env::var("STRATEGY_MODE").unwrap_or_else(|_| "standard".to_string());
        let strategy_mode = StrategyMode::from_str(&strategy_mode_str)?;

        // Parse Risk Appetite first (may override other values)
        let risk_appetite = if let Ok(score_str) = env::var("RISK_APPETITE_SCORE") {
            let score = score_str
                .parse::<u8>()
                .context("Failed to parse RISK_APPETITE_SCORE - must be integer 1-9")?;
            Some(RiskAppetite::new(score).context("RISK_APPETITE_SCORE must be between 1 and 9")?)
        } else {
            None
        };

        // Base values from env
        let rsi_threshold_base = Self::parse_f64("RSI_THRESHOLD", 75.0)?;
        let trailing_stop_base = Self::parse_f64("TRAILING_STOP_ATR_MULTIPLIER", 5.0)?;
        let macd_requires_rising_base = true;
        let trend_tolerance_base = 0.0;
        let macd_min_threshold_base = 0.0;
        let profit_target_base = 1.5;

        // Apply risk appetite overrides if set
        let (
            rsi_threshold,
            trailing_stop_atr_multiplier,
            macd_requires_rising,
            trend_tolerance_pct,
            macd_min_threshold,
            profit_target_multiplier,
        ) = if let Some(ref appetite) = risk_appetite {
            (
                appetite.calculate_rsi_threshold(),
                appetite.calculate_trailing_stop_multiplier(),
                appetite.requires_macd_rising(),
                appetite.calculate_trend_tolerance_pct(),
                appetite.calculate_macd_min_threshold(),
                appetite.calculate_profit_target_multiplier(),
            )
        } else {
            (
                rsi_threshold_base,
                trailing_stop_base,
                macd_requires_rising_base,
                trend_tolerance_base,
                macd_min_threshold_base,
                profit_target_base,
            )
        };

        // Multi-Timeframe
        let primary_timeframe = env::var("PRIMARY_TIMEFRAME")
            .unwrap_or_else(|_| "1Min".to_string())
            .parse::<Timeframe>()
            .context("Failed to parse PRIMARY_TIMEFRAME")?;

        let timeframes_str =
            env::var("TIMEFRAMES").unwrap_or_else(|_| "1Min,5Min,15Min,1Hour".to_string());
        let enabled_timeframes: Vec<Timeframe> = timeframes_str
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse TIMEFRAMES")?;

        let trend_timeframe = env::var("TREND_TIMEFRAME")
            .unwrap_or_else(|_| "1Hour".to_string())
            .parse::<Timeframe>()
            .context("Failed to parse TREND_TIMEFRAME")?;

        Ok(Self {
            fast_sma_period: Self::parse_usize("FAST_SMA_PERIOD", 20)?,
            slow_sma_period: Self::parse_usize("SLOW_SMA_PERIOD", 60)?,
            trend_sma_period: Self::parse_usize("TREND_SMA_PERIOD", 200)?,
            sma_threshold: Self::parse_f64("SMA_THRESHOLD", 0.001)?,
            rsi_period: Self::parse_usize("RSI_PERIOD", 14)?,
            rsi_threshold,
            macd_fast_period: Self::parse_usize("MACD_FAST_PERIOD", 12)?,
            macd_slow_period: Self::parse_usize("MACD_SLOW_PERIOD", 26)?,
            macd_signal_period: Self::parse_usize("MACD_SIGNAL_PERIOD", 9)?,
            macd_requires_rising,
            macd_min_threshold,
            ema_fast_period: Self::parse_usize("EMA_FAST_PERIOD", 50).unwrap_or(50),
            ema_slow_period: Self::parse_usize("EMA_SLOW_PERIOD", 150).unwrap_or(150),
            adx_period: Self::parse_usize("ADX_PERIOD", 14).unwrap_or(14),
            adx_threshold: Self::parse_f64("ADX_THRESHOLD", 25.0).unwrap_or(25.0),
            atr_period: Self::parse_usize("ATR_PERIOD", 14)?,
            trailing_stop_atr_multiplier,
            strategy_mode,
            trend_divergence_threshold: Self::parse_f64("TREND_DIVERGENCE_THRESHOLD", 0.005)?,
            trend_tolerance_pct,
            mean_reversion_rsi_exit: Self::parse_f64("MEAN_REVERSION_RSI_EXIT", 50.0)?,
            mean_reversion_bb_period: Self::parse_usize("MEAN_REVERSION_BB_PERIOD", 20)?,
            trend_riding_exit_buffer_pct: Self::parse_f64("TREND_RIDING_EXIT_BUFFER_PCT", 0.03)?,
            smc_ob_lookback: Self::parse_usize("SMC_OB_LOOKBACK", 20).unwrap_or(20),
            smc_min_fvg_size_pct: Self::parse_f64("SMC_MIN_FVG_SIZE_PCT", 0.005).unwrap_or(0.005),
            primary_timeframe,
            enabled_timeframes,
            trend_timeframe,
            signal_confirmation_bars: Self::parse_usize("SIGNAL_CONFIRMATION_BARS", 2)?,
            take_profit_pct: Self::parse_f64("TAKE_PROFIT_PCT", 0.05).unwrap_or(0.05),
            profit_target_multiplier,
            risk_appetite,
            enable_ml_data_collection: env::var("ENABLE_ML_DATA_COLLECTION")
                .unwrap_or_else(|_| "false".to_string())
                .parse::<bool>()
                .unwrap_or(false),
        })
    }

    fn parse_usize(key: &str, default: usize) -> Result<usize> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<usize>()
            .context(format!("Failed to parse {}", key))
    }

    fn parse_f64(key: &str, default: f64) -> Result<f64> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<f64>()
            .context(format!("Failed to parse {}", key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_config_defaults() {
        let config = StrategyEnvConfig::from_env().expect("Should parse with defaults");
        assert_eq!(config.fast_sma_period, 20);
        assert_eq!(config.slow_sma_period, 60);
        assert_eq!(config.rsi_period, 14);
    }
}
