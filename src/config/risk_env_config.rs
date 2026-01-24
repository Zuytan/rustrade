//! Risk management configuration parsing from environment variables.
//!
//! This module handles loading risk parameters: position sizing, drawdown limits,
//! PDT rules, sector exposure, and transaction costs.

use crate::domain::risk::risk_appetite::RiskAppetite;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::collections::HashMap;
use std::env;

/// Risk management environment configuration
#[derive(Debug, Clone)]
pub struct RiskEnvConfig {
    // Position Sizing
    pub max_positions: usize,
    pub max_position_size_pct: f64,
    pub max_position_value_usd: f64,
    pub risk_per_trade_percent: f64,

    // Drawdown & Circuit Breaker
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub consecutive_loss_limit: usize,
    pub pending_order_ttl_ms: Option<i64>,

    // Sector Exposure
    pub max_sector_exposure_pct: f64,
    pub sector_map: HashMap<String, String>,

    // PDT
    pub non_pdt_mode: bool,

    // Trading Limits
    pub max_orders_per_minute: u32,
    pub order_cooldown_seconds: u64,
    pub min_hold_time_minutes: i64,

    // Transaction Costs
    pub slippage_pct: f64,
    pub commission_per_share: f64,
    pub spread_bps: f64,
    pub min_profit_ratio: f64,

    // Portfolio Management
    pub trade_quantity: Decimal,
    pub portfolio_staleness_ms: u64,
    pub portfolio_refresh_interval_ms: u64,

    // Dynamic Symbol Mode
    pub dynamic_symbol_mode: bool,
    pub dynamic_scan_interval_minutes: u64,
    pub symbols: Vec<String>,
    pub min_volume_threshold: f64,

    // Adaptive Optimization
    pub adaptive_optimization_enabled: bool,
    pub regime_detection_window: usize,
    pub adaptive_evaluation_hour: u32,

    // Risk Appetite (for derived values)
    risk_appetite: Option<RiskAppetite>,
}

impl RiskEnvConfig {
    pub fn from_env() -> Result<Self> {
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
        let risk_per_trade_base = Self::parse_f64("RISK_PER_TRADE_PERCENT", 0.015)?;
        let max_position_size_base = Self::parse_f64("MAX_POSITION_SIZE_PCT", 0.1)?;
        let min_profit_ratio_base = Self::parse_f64("MIN_PROFIT_RATIO", 2.0).unwrap_or(2.0);

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
                Self::parse_f64("MAX_DAILY_LOSS_PCT", 0.02)?,
                Self::parse_f64("MAX_DRAWDOWN_PCT", 0.1)?,
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

        let trade_quantity_f64 = Self::parse_f64("TRADE_QUANTITY", 1.0)?;
        let trade_quantity =
            Decimal::from_f64(trade_quantity_f64).unwrap_or_else(|| Decimal::from(1));

        Ok(Self {
            max_positions: Self::parse_usize("MAX_POSITIONS", 5)?,
            max_position_size_pct,
            max_position_value_usd: Self::parse_f64("MAX_POSITION_VALUE_USD", 5000.0)?,
            risk_per_trade_percent,
            max_daily_loss_pct,
            max_drawdown_pct,
            consecutive_loss_limit: Self::parse_usize("CONSECUTIVE_LOSS_LIMIT", 3)?,
            pending_order_ttl_ms: env::var("PENDING_ORDER_TTL_MS")
                .ok()
                .and_then(|s| s.parse::<i64>().ok()),
            max_sector_exposure_pct: Self::parse_f64("MAX_SECTOR_EXPOSURE_PCT", 0.30)?,
            sector_map,
            non_pdt_mode: Self::parse_bool("NON_PDT_MODE", true),
            max_orders_per_minute: Self::parse_u32("MAX_ORDERS_PER_MINUTE", 10)?,
            order_cooldown_seconds: Self::parse_u64("ORDER_COOLDOWN_SECONDS", 300)?,
            min_hold_time_minutes: Self::parse_i64("MIN_HOLD_TIME_MINUTES", 240)?,
            slippage_pct: Self::parse_f64("SLIPPAGE_PCT", 0.001)?,
            commission_per_share: Self::parse_f64("COMMISSION_PER_SHARE", 0.001)?,
            spread_bps: Self::parse_f64("SPREAD_BPS", 5.0).unwrap_or(5.0),
            min_profit_ratio,
            trade_quantity,
            portfolio_staleness_ms: Self::parse_u64("PORTFOLIO_STALENESS_MS", 5000).unwrap_or(5000),
            portfolio_refresh_interval_ms: Self::parse_u64("PORTFOLIO_REFRESH_INTERVAL_MS", 2000)
                .unwrap_or(2000),
            dynamic_symbol_mode,
            dynamic_scan_interval_minutes: Self::parse_u64("DYNAMIC_SCAN_INTERVAL_MINUTES", 5)?,
            symbols,
            min_volume_threshold: Self::parse_f64("MIN_VOLUME_THRESHOLD", 50000.0)
                .unwrap_or(50000.0),
            adaptive_optimization_enabled: Self::parse_bool("ADAPTIVE_OPTIMIZATION_ENABLED", false),
            regime_detection_window: Self::parse_usize("REGIME_DETECTION_WINDOW", 20).unwrap_or(20),
            adaptive_evaluation_hour: Self::parse_u32("ADAPTIVE_EVALUATION_HOUR", 0).unwrap_or(0),
            risk_appetite,
        })
    }

    pub fn risk_appetite(&self) -> Option<&RiskAppetite> {
        self.risk_appetite.as_ref()
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
            .unwrap_or_else(|_| default.to_string())
            .parse::<bool>()
            .unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_config_defaults() {
        let config = RiskEnvConfig::from_env().expect("Should parse with defaults");
        assert_eq!(config.max_positions, 5);
        assert_eq!(config.consecutive_loss_limit, 3);
    }
}
