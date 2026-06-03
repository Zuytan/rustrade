use crate::domain::ports::SectorProvider;
use crate::domain::risk::filters::correlation_filter::CorrelationFilterConfig;
use crate::domain::risk::volatility_manager::VolatilityConfig;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Error type for RiskManager configuration validation
#[derive(Debug, thiserror::Error)]
pub enum RiskConfigError {
    #[error("Invalid RiskConfig: {0}")]
    ValidationError(String),
}

/// Risk management configuration
#[derive(Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub max_position_size_pct: Decimal, // Max % of equity per position (e.g., 0.25 = 25%)
    pub max_daily_loss_pct: Decimal,    // Max % loss per day (e.g., 0.02 = 2%)
    pub max_drawdown_pct: Decimal,      // Max % drawdown from high water mark (e.g., 0.10 = 10%)
    pub consecutive_loss_limit: usize,  // Max consecutive losing trades before halt
    pub valuation_interval_seconds: u64, // Interval for portfolio valuation check
    pub max_sector_exposure_pct: Decimal, // Max exposure per sector
    #[serde(skip)]
    pub sector_provider: Option<Arc<dyn SectorProvider>>,
    pub allow_pdt_risk: bool, // If true, allows opening orders even if PDT saturated (Risky!)
    pub pending_order_ttl_ms: Option<i64>, // TTL for pending orders filled but not synced
    pub correlation_config: CorrelationFilterConfig,
    pub volatility_config: VolatilityConfig, // Added

    // --- Execution Risk & Position Management ---
    pub max_positions: usize, // Maximum number of concurrent positions
    pub risk_per_trade_percent: Decimal, // Risk per trade as percentage of capital
    pub trade_quantity: Decimal, // Default quantity for trades if not risk-scaled
    pub order_cooldown_seconds: u64, // Cooldown period between orders for the same symbol
    pub max_orders_per_minute: u32, // Maximum orders allowed per minute (Throttling)
    pub min_hold_time_minutes: i64, // Minimum hold time before a position can be closed
    pub max_loss_per_trade_pct: Decimal, // Maximum allowed loss per single trade (Hard stop)
}

impl std::fmt::Debug for RiskConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RiskConfig")
            .field("max_position_size_pct", &self.max_position_size_pct)
            .field("max_daily_loss_pct", &self.max_daily_loss_pct)
            .field("max_drawdown_pct", &self.max_drawdown_pct)
            .field("consecutive_loss_limit", &self.consecutive_loss_limit)
            .field(
                "valuation_interval_seconds",
                &self.valuation_interval_seconds,
            )
            .field("max_sector_exposure_pct", &self.max_sector_exposure_pct)
            .field("allow_pdt_risk", &self.allow_pdt_risk)
            .field("pending_order_ttl_ms", &self.pending_order_ttl_ms)
            .field("correlation_config", &self.correlation_config)
            .field("volatility_config", &self.volatility_config)
            .field("max_positions", &self.max_positions)
            .field("risk_per_trade_percent", &self.risk_per_trade_percent)
            .field("trade_quantity", &self.trade_quantity)
            .field("order_cooldown_seconds", &self.order_cooldown_seconds)
            .field("max_orders_per_minute", &self.max_orders_per_minute)
            .field("min_hold_time_minutes", &self.min_hold_time_minutes)
            .field("max_loss_per_trade_pct", &self.max_loss_per_trade_pct)
            .finish()
    }
}

impl RiskConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_position_size_pct <= Decimal::ZERO || self.max_position_size_pct > Decimal::ONE
        {
            return Err(format!(
                "Invalid max_position_size_pct: {}",
                self.max_position_size_pct
            ));
        }
        if self.max_daily_loss_pct <= Decimal::ZERO || self.max_daily_loss_pct > dec!(0.5) {
            return Err(format!(
                "Invalid max_daily_loss_pct: {}",
                self.max_daily_loss_pct
            ));
        }
        if self.max_drawdown_pct <= Decimal::ZERO || self.max_drawdown_pct > Decimal::ONE {
            return Err(format!(
                "Invalid max_drawdown_pct: {}",
                self.max_drawdown_pct
            ));
        }
        if self.consecutive_loss_limit == 0 {
            return Err("consecutive_loss_limit must be > 0".to_string());
        }
        if self.max_sector_exposure_pct <= Decimal::ZERO
            || self.max_sector_exposure_pct > Decimal::ONE
        {
            return Err(format!(
                "Invalid max_sector_exposure_pct: {}",
                self.max_sector_exposure_pct
            ));
        }
        if self.max_positions == 0 {
            return Err("max_positions must be > 0".to_string());
        }
        if self.risk_per_trade_percent < Decimal::ZERO || self.risk_per_trade_percent > Decimal::ONE
        {
            return Err(format!(
                "Invalid risk_per_trade_percent: {}",
                self.risk_per_trade_percent
            ));
        }
        if let Some(ttl) = self.pending_order_ttl_ms
            && ttl <= 0
        {
            return Err("pending_order_ttl_ms must be > 0".to_string());
        }
        Ok(())
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_size_pct: dec!(0.10), // Reduced from 0.25 for safety
            max_daily_loss_pct: dec!(0.02),    // 2%
            max_drawdown_pct: dec!(0.05),      // Reduced from 0.10 for safety
            consecutive_loss_limit: 3,
            valuation_interval_seconds: 60,
            max_sector_exposure_pct: dec!(0.20), // Reduced from 0.30

            sector_provider: None,
            allow_pdt_risk: false,
            pending_order_ttl_ms: None, // Default 5 mins
            correlation_config: CorrelationFilterConfig::default(),
            volatility_config: VolatilityConfig::default(),

            max_positions: 5,
            risk_per_trade_percent: dec!(0.01), // 1%
            trade_quantity: Decimal::ONE,
            order_cooldown_seconds: 60,
            max_orders_per_minute: 10,
            min_hold_time_minutes: 0,
            max_loss_per_trade_pct: dec!(-0.05), // -5%
        }
    }
}

impl RiskConfig {
    /// Crypto-oriented defaults: higher drawdown/position limits and more consecutive losses before halt.
    pub fn crypto_default() -> Self {
        Self {
            max_position_size_pct: dec!(0.15), // 15%
            max_daily_loss_pct: dec!(0.04),    // 4%
            max_drawdown_pct: dec!(0.12),      // 12%
            consecutive_loss_limit: 6,
            valuation_interval_seconds: 60,
            max_sector_exposure_pct: dec!(0.20),
            sector_provider: None,
            allow_pdt_risk: false,
            pending_order_ttl_ms: None,
            correlation_config: CorrelationFilterConfig::default(),
            volatility_config: VolatilityConfig::default(),

            max_positions: 10,
            risk_per_trade_percent: dec!(0.02), // 2%
            trade_quantity: Decimal::ONE,
            order_cooldown_seconds: 30,
            max_orders_per_minute: 20,
            min_hold_time_minutes: 0,
            max_loss_per_trade_pct: dec!(-0.10), // -10%
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_default_relaxed_limits() {
        let default = RiskConfig::default();
        let crypto = RiskConfig::crypto_default();
        assert!(crypto.max_drawdown_pct > default.max_drawdown_pct);
        assert!(crypto.max_position_size_pct > default.max_position_size_pct);
        assert!(crypto.consecutive_loss_limit > default.consecutive_loss_limit);
        assert!(crypto.max_daily_loss_pct > default.max_daily_loss_pct);
        assert!(crypto.validate().is_ok());
    }
}
