//! Strategy configuration parsing from environment variables.
//!
//! This module handles loading technical indicator and strategy parameters.

use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::market::timeframe::Timeframe;
use crate::domain::risk::risk_appetite::RiskAppetite;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::env;
use std::str::FromStr;

pub struct StrategyEnvLoader;

impl StrategyEnvLoader {
    pub fn from_env() -> Result<crate::domain::config::StrategyConfig> {
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
        let rsi_threshold_base = Self::parse_decimal("RSI_THRESHOLD", dec!(75.0))?;
        let trailing_stop_base = Self::parse_decimal("TRAILING_STOP_ATR_MULTIPLIER", dec!(5.0))?;
        let macd_requires_rising_base = true;
        let trend_tolerance_base = Decimal::ZERO;
        let macd_min_threshold_base = Decimal::ZERO;
        let profit_target_base = dec!(1.5);

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

        let config = crate::domain::config::StrategyConfig {
            strategy_mode,
            fast_sma_period: Self::parse_usize("FAST_SMA_PERIOD", 20)?,
            slow_sma_period: Self::parse_usize("SLOW_SMA_PERIOD", 60)?,
            trend_sma_period: Self::parse_usize("TREND_SMA_PERIOD", 200)?,
            sma_threshold: Self::parse_decimal("SMA_THRESHOLD", dec!(0.001))?,
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
            adx_threshold: Self::parse_decimal("ADX_THRESHOLD", dec!(25.0)).unwrap_or(dec!(25.0)),
            regime_volatility_threshold: Self::parse_decimal(
                "REGIME_VOLATILITY_THRESHOLD",
                dec!(2.0),
            )
            .unwrap_or(dec!(2.0)),
            bb_std_dev: Self::parse_decimal("BB_STD_DEV", dec!(2.0)).unwrap_or(dec!(2.0)),
            spread_bps: Self::parse_decimal("SPREAD_BPS", dec!(0.5)).unwrap_or(dec!(0.5)),
            atr_period: Self::parse_usize("ATR_PERIOD", 14)?,
            trailing_stop_atr_multiplier,
            min_profit_ratio: Self::parse_decimal("MIN_PROFIT_RATIO", dec!(1.5))?,
            take_profit_pct: Self::parse_decimal("TAKE_PROFIT_PCT", dec!(0.05))
                .unwrap_or(dec!(0.05)),
            profit_target_multiplier,
            trend_divergence_threshold: Self::parse_decimal(
                "TREND_DIVERGENCE_THRESHOLD",
                dec!(0.005),
            )?,
            trend_tolerance_pct,
            trend_riding_exit_buffer_pct: Self::parse_decimal(
                "TREND_RIDING_EXIT_BUFFER_PCT",
                dec!(0.03),
            )?,
            mean_reversion_rsi_exit: Self::parse_decimal("MEAN_REVERSION_RSI_EXIT", dec!(50.0))?,
            mean_reversion_bb_period: Self::parse_usize("MEAN_REVERSION_BB_PERIOD", 20)?,
            signal_confirmation_bars: Self::parse_usize("SIGNAL_CONFIRMATION_BARS", 2)?,
            smc_ob_lookback: Self::parse_usize("SMC_OB_LOOKBACK", 20).unwrap_or(20),
            smc_min_fvg_size_pct: Self::parse_decimal("SMC_MIN_FVG_SIZE_PCT", dec!(0.005))
                .unwrap_or(dec!(0.005)),
            smc_volume_multiplier: Self::parse_decimal("SMC_VOLUME_MULTIPLIER", dec!(1.5))
                .unwrap_or(dec!(1.5)),
            breakout_lookback: Self::parse_usize("BREAKOUT_LOOKBACK", 20).unwrap_or(20),
            breakout_threshold_pct: Self::parse_decimal("BREAKOUT_THRESHOLD_PCT", dec!(0.0005))
                .unwrap_or(dec!(0.0005)),
            breakout_volume_mult: Self::parse_decimal("BREAKOUT_VOLUME_MULT", dec!(0.1))
                .unwrap_or(dec!(0.1)),
            stat_momentum_lookback: Self::parse_usize("STAT_MOMENTUM_LOOKBACK", 10).unwrap_or(10),
            stat_momentum_threshold: Self::parse_decimal("STAT_MOMENTUM_THRESHOLD", dec!(0.8))
                .unwrap_or(dec!(0.8)),
            stat_momentum_trend_confirmation: Self::parse_bool(
                "STAT_MOMENTUM_TREND_CONFIRMATION",
                true,
            ),
            zscore_lookback: Self::parse_usize("ZSCORE_LOOKBACK", 20).unwrap_or(20),
            zscore_entry_threshold: Self::parse_decimal("ZSCORE_ENTRY_THRESHOLD", dec!(-1.5))
                .unwrap_or(dec!(-1.5)),
            zscore_exit_threshold: Self::parse_decimal("ZSCORE_EXIT_THRESHOLD", dec!(0.0))
                .unwrap_or(dec!(0.0)),
            orderflow_ofi_threshold: Self::parse_decimal("ORDERFLOW_OFI_THRESHOLD", dec!(0.3))
                .unwrap_or(dec!(0.3)),
            orderflow_stacked_count: Self::parse_usize("ORDERFLOW_STACKED_COUNT", 3).unwrap_or(3),
            orderflow_volume_profile_lookback: Self::parse_usize(
                "ORDERFLOW_VOLUME_PROFILE_LOOKBACK",
                100,
            )
            .unwrap_or(100),
            ensemble_weights: None, // Cannot easily parse weights from env yet
            ensemble_voting_threshold: Self::parse_decimal(
                "ENSEMBLE_VOTING_THRESHOLD",
                dec!(0.50),
            )?,
            primary_timeframe,
            enabled_timeframes,
            trend_timeframe,
            enable_ml_data_collection: env::var("ENABLE_ML_DATA_COLLECTION")
                .unwrap_or_else(|_| "false".to_string())
                .parse::<bool>()
                .unwrap_or(false),
            risk_appetite_score: risk_appetite.map(|a| a.score()),
        };

        config
            .validate()
            .map_err(|e| anyhow::anyhow!("Strategy validation failed: {}", e))?;
        Ok(config)
    }

    fn parse_usize(key: &str, default: usize) -> Result<usize> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<usize>()
            .context(format!("Failed to parse {}", key))
    }

    fn parse_decimal(key: &str, default: Decimal) -> Result<Decimal> {
        match env::var(key) {
            Ok(val) => val
                .parse::<Decimal>()
                .context(format!("Failed to parse {} as Decimal", key)),
            Err(_) => Ok(default),
        }
    }

    fn parse_bool(key: &str, default: bool) -> bool {
        env::var(key)
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_config_defaults() {
        let config = StrategyEnvLoader::from_env().expect("Should parse with defaults");
        assert_eq!(config.fast_sma_period, 20);
        assert_eq!(config.slow_sma_period, 60);
        assert_eq!(config.rsi_period, 14);
    }
}
