//! Risk management configuration parsing from environment variables.
//!
//! This module handles loading risk parameters: position sizing, drawdown limits,
//! PDT rules, sector exposure, and transaction costs.

use crate::domain::risk::risk_appetite::RiskAppetite;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::env;

/// Platform-level settings that are not part of pure trading risk logic
#[derive(Debug, Clone, Default)]
pub struct PlatformConfig {
    pub max_position_value_usd: Decimal,
    pub sector_map: HashMap<String, String>,
    pub non_pdt_mode: bool,
    pub slippage_pct: Decimal,
    pub commission_per_share: Decimal,
    pub spread_bps: Decimal,
    pub min_profit_ratio: Decimal,
    pub portfolio_staleness_ms: u64,
    pub portfolio_refresh_interval_ms: u64,
    pub dynamic_symbol_mode: bool,
    pub dynamic_scan_interval_minutes: u64,
    pub symbols: Vec<String>,
    pub min_volume_threshold: Decimal,
    pub adaptive_optimization_enabled: bool,
    pub regime_detection_window: usize,
    pub adaptive_evaluation_hour: u32,
    pub use_real_market_data: bool,
}

pub struct RiskEnvLoader;

impl RiskEnvLoader {
    pub fn from_env() -> Result<(crate::domain::risk::risk_config::RiskConfig, PlatformConfig)> {
        use rust_decimal_macros::dec;
        // Parse Risk Appetite first
        let risk_appetite = if let Ok(score_str) = env::var("RISK_APPETITE_SCORE") {
            let score = score_str
                .parse::<u8>()
                .context("Failed to parse RISK_APPETITE_SCORE")?;
            Some(RiskAppetite::new(score).context("RISK_APPETITE_SCORE must be between 1 and 9")?)
        } else {
            None
        };

        // Base values
        let risk_per_trade_base = Self::parse_decimal("RISK_PER_TRADE_PERCENT", dec!(0.015))?;
        let max_position_size_base = Self::parse_decimal("MAX_POSITION_SIZE_PCT", dec!(0.1))?;
        let min_profit_ratio_base = Self::parse_decimal("MIN_PROFIT_RATIO", dec!(2.0))?;

        // Apply risk appetite overrides
        let (
            risk_per_trade_percent,
            max_position_size_pct,
            min_profit_ratio,
            max_daily_loss_pct,
            max_drawdown_pct,
        ) = if let Some(ref appetite) = risk_appetite {
            (
                appetite.calculate_risk_per_trade_percent(),
                appetite.calculate_max_position_size_pct(),
                appetite.calculate_min_profit_ratio(),
                appetite.calculate_max_daily_loss_pct(), // Override default
                appetite.calculate_max_drawdown_pct(),   // Override default
            )
        } else {
            (
                risk_per_trade_base,
                max_position_size_base,
                min_profit_ratio_base,
                Self::parse_decimal("MAX_DAILY_LOSS_PCT", dec!(0.02))?,
                Self::parse_decimal("MAX_DRAWDOWN_PCT", dec!(0.1))?,
            )
        };

        // Dynamic symbol mode
        let dynamic_symbol_mode = Self::parse_bool("DYNAMIC_SYMBOL_MODE", false);
        let symbols_default = if dynamic_symbol_mode { "" } else { "AAPL" };
        let symbols_str = env::var("SYMBOLS").unwrap_or_else(|_| symbols_default.to_string());
        let symbols: Vec<String> = if symbols_str.is_empty() {
            vec![]
        } else {
            symbols_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        };

        // Sector map
        let sectors_env = env::var("SECTORS").unwrap_or_default();
        let mut sector_map = HashMap::new();
        for entry in sectors_env.split(',') {
            if let Some((sym, sec)) = entry.split_once(':') {
                sector_map.insert(sym.trim().to_string(), sec.trim().to_string());
            }
        }

        let trade_quantity = Self::parse_decimal("TRADE_QUANTITY", dec!(1.0))?;

        let risk_config = crate::domain::risk::risk_config::RiskConfig {
            max_position_size_pct,
            max_sector_exposure_pct: Self::parse_decimal("MAX_SECTOR_EXPOSURE_PCT", dec!(0.30))?,
            max_daily_loss_pct,
            max_drawdown_pct,
            consecutive_loss_limit: Self::parse_usize("CONSECUTIVE_LOSS_LIMIT", 3)?,
            valuation_interval_seconds: Self::parse_u64("VALUATION_INTERVAL_SECONDS", 60)?,
            sector_provider: None,
            allow_pdt_risk: Self::parse_bool("ALLOW_PDT_RISK", false),
            pending_order_ttl_ms: env::var("PENDING_ORDER_TTL_MS")
                .ok()
                .and_then(|s| s.parse::<i64>().ok()),
            correlation_config:
                crate::domain::risk::filters::correlation_filter::CorrelationFilterConfig::default(),
            volatility_config: crate::domain::risk::volatility_manager::VolatilityConfig::default(),
            max_positions: Self::parse_usize("MAX_POSITIONS", 5)?,
            risk_per_trade_percent,
            trade_quantity,
            order_cooldown_seconds: Self::parse_u64("ORDER_COOLDOWN_SECONDS", 300)?,
            max_orders_per_minute: Self::parse_u32("MAX_ORDERS_PER_MINUTE", 10)?,
            min_hold_time_minutes: Self::parse_i64("MIN_HOLD_TIME_MINUTES", 240)?,
            max_loss_per_trade_pct: Self::parse_decimal("MAX_LOSS_PER_TRADE_PCT", dec!(-0.05))?,
        };

        risk_config
            .validate()
            .map_err(|e| anyhow::anyhow!("Risk validation failed: {}", e))?;

        let platform_config = PlatformConfig {
            max_position_value_usd: Self::parse_decimal("MAX_POSITION_VALUE_USD", dec!(5000.0))?,
            sector_map,
            non_pdt_mode: Self::parse_bool("NON_PDT_MODE", true),
            slippage_pct: Self::parse_decimal("SLIPPAGE_PCT", dec!(0.001))?,
            commission_per_share: Self::parse_decimal("COMMISSION_PER_SHARE", dec!(0.001))?,
            spread_bps: Self::parse_decimal("SPREAD_BPS", dec!(5.0))?,
            min_profit_ratio,
            portfolio_staleness_ms: Self::parse_u64("PORTFOLIO_STALENESS_MS", 5000)?,
            portfolio_refresh_interval_ms: Self::parse_u64("PORTFOLIO_REFRESH_INTERVAL_MS", 2000)?,
            dynamic_symbol_mode,
            dynamic_scan_interval_minutes: Self::parse_u64("DYNAMIC_SCAN_INTERVAL_MINUTES", 5)?,
            symbols,
            min_volume_threshold: Self::parse_decimal("MIN_VOLUME_THRESHOLD", dec!(50000.0))?,
            adaptive_optimization_enabled: Self::parse_bool("ADAPTIVE_OPTIMIZATION_ENABLED", false),
            regime_detection_window: Self::parse_usize("REGIME_DETECTION_WINDOW", 20)?,
            adaptive_evaluation_hour: Self::parse_u32("ADAPTIVE_EVALUATION_HOUR", 0)?,
            use_real_market_data: Self::parse_bool("USE_REAL_MARKET_DATA", false),
        };

        Ok((risk_config, platform_config))
    }

    fn parse_usize(key: &str, default: usize) -> Result<usize> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<usize>()
            .context(format!("Failed to parse {}", key))
    }

    fn parse_decimal(key: &str, default: Decimal) -> Result<Decimal> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<Decimal>()
            .map_err(|_| anyhow::anyhow!("Failed to parse {} as Decimal", key))
    }

    fn parse_u32(key: &str, default: u32) -> Result<u32> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<u32>()
            .context(format!("Failed to parse {}", key))
    }

    fn parse_u64(key: &str, default: u64) -> Result<u64> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<u64>()
            .context(format!("Failed to parse {}", key))
    }

    fn parse_i64(key: &str, default: i64) -> Result<i64> {
        env::var(key)
            .unwrap_or_else(|_| default.to_string())
            .parse::<i64>()
            .context(format!("Failed to parse {}", key))
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
    fn test_risk_config_defaults() {
        let (risk, platform) = RiskEnvLoader::from_env().expect("Should parse with defaults");
        assert_eq!(risk.max_positions, 5);
        assert_eq!(risk.consecutive_loss_limit, 3);
        assert!(platform.non_pdt_mode);
    }
}
