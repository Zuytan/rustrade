//! Risk Configuration Domain Value Object
//!
//! This module defines the `RiskConfig` value object, which encapsulates
//! all risk management parameters with validation logic.
//!
//! # Design Principles
//!
//! - **Immutability**: All fields are public but the struct is validated on construction
//! - **Self-Validation**: The `validate()` method ensures invariants are maintained
//! - **Domain Logic**: Percentage validations belong in the domain, not infrastructure

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error type for RiskConfig validation
#[derive(Debug, Error, PartialEq)]
pub enum RiskConfigError {
    #[error("Invalid percentage: {field} = {value}. Must be between 0.0 and 1.0")]
    InvalidPercentage { field: String, value: Decimal },

    #[error("Invalid limit: {field} = {value}. Must be positive")]
    InvalidLimit { field: String, value: usize },

    #[error("Invalid TTL: {field} = {value}. Must be positive")]
    InvalidTtl { field: String, value: i64 },
}

/// Risk management configuration value object
///
/// # Invariants
///
/// - All percentage fields must be in range [0.0, 1.0]
/// - `consecutive_loss_limit` must be > 0
/// - `pending_order_ttl_ms` (if set) must be > 0
///
/// # Example
///
/// ```rust
/// use rustrade::domain::config::RiskConfig;
/// use rust_decimal_macros::dec;
///
/// let config = RiskConfig {
///     max_position_size_pct: dec!(0.1),
///     max_sector_exposure_pct: dec!(0.3),
///     max_daily_loss_pct: dec!(0.02),
///     max_drawdown_pct: dec!(0.1),
///     consecutive_loss_limit: 3,
///     pending_order_ttl_ms: Some(5000),
///     ..RiskConfig::default()
/// };
/// assert!(config.validate().is_ok());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Maximum position size as percentage of portfolio (e.g., 0.1 = 10%)
    pub max_position_size_pct: Decimal,

    /// Maximum sector exposure as percentage of portfolio (e.g., 0.3 = 30%)
    pub max_sector_exposure_pct: Decimal,

    /// Maximum daily loss as percentage of portfolio (e.g., 0.02 = 2%)
    pub max_daily_loss_pct: Decimal,

    /// Maximum drawdown from high-water mark as percentage (e.g., 0.1 = 10%)
    pub max_drawdown_pct: Decimal,

    /// Maximum consecutive losses before halting trading
    pub consecutive_loss_limit: usize,

    /// Time-to-live for pending orders in milliseconds (None = no expiration)
    pub pending_order_ttl_ms: Option<i64>,

    // --- Execution Risk & Position Management ---
    /// Maximum number of concurrent positions
    pub max_positions: usize,

    /// Risk per trade as percentage of capital (e.g., 0.01 = 1%)
    pub risk_per_trade_percent: Decimal,

    /// Default quantity for trades if not risk-scaled
    pub trade_quantity: Decimal,

    /// Cooldown period between orders for the same symbol
    pub order_cooldown_seconds: u64,

    /// Maximum orders allowed per minute (Throttling)
    pub max_orders_per_minute: u32,

    /// Minimum hold time before a position can be closed (Safety)
    pub min_hold_time_minutes: i64,

    /// Maximum allowed loss per single trade (Hard stop)
    pub max_loss_per_trade_pct: Decimal,
}

impl RiskConfig {
    /// Create a new RiskConfig with validation.
    /// Invariants are maintained by calling validate() after initialization.
    pub fn validate(&self) -> Result<(), RiskConfigError> {
        // Validate percentages
        self.validate_percentage("max_position_size_pct", self.max_position_size_pct)?;
        self.validate_percentage("max_sector_exposure_pct", self.max_sector_exposure_pct)?;
        self.validate_percentage("max_daily_loss_pct", self.max_daily_loss_pct)?;
        self.validate_percentage("max_drawdown_pct", self.max_drawdown_pct)?;
        self.validate_percentage("risk_per_trade_percent", self.risk_per_trade_percent)?;

        // Validate consecutive loss limit
        if self.consecutive_loss_limit == 0 {
            return Err(RiskConfigError::InvalidLimit {
                field: "consecutive_loss_limit".to_string(),
                value: self.consecutive_loss_limit,
            });
        }

        // Validate TTL if present
        if let Some(ttl) = self.pending_order_ttl_ms
            && ttl <= 0
        {
            return Err(RiskConfigError::InvalidTtl {
                field: "pending_order_ttl_ms".to_string(),
                value: ttl,
            });
        }

        // Validate execution limits
        if self.max_positions == 0 {
            return Err(RiskConfigError::InvalidLimit {
                field: "max_positions".to_string(),
                value: self.max_positions,
            });
        }

        Ok(())
    }

    /// Validate a percentage field is in range [0.0, 1.0]
    fn validate_percentage(&self, field: &str, value: Decimal) -> Result<(), RiskConfigError> {
        use rust_decimal_macros::dec;
        if value < Decimal::ZERO || value > dec!(1.0) {
            return Err(RiskConfigError::InvalidPercentage {
                field: field.to_string(),
                value,
            });
        }
        Ok(())
    }

    // Removed converting methods as fields are now Decimal
}

impl Default for RiskConfig {
    /// Conservative default risk parameters
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            max_position_size_pct: dec!(0.1),   // 10%
            max_sector_exposure_pct: dec!(0.3), // 30%
            max_daily_loss_pct: dec!(0.02),     // 2%
            max_drawdown_pct: dec!(0.1),        // 10%
            consecutive_loss_limit: 3,
            pending_order_ttl_ms: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_valid_config() {
        use rust_decimal_macros::dec;
        let config = RiskConfig {
            max_position_size_pct: dec!(0.1),
            max_sector_exposure_pct: dec!(0.3),
            max_daily_loss_pct: dec!(0.02),
            max_drawdown_pct: dec!(0.1),
            consecutive_loss_limit: 3,
            pending_order_ttl_ms: Some(5000),
            max_positions: 5,
            risk_per_trade_percent: dec!(0.01),
            trade_quantity: dec!(1.0),
            order_cooldown_seconds: 60,
            max_orders_per_minute: 10,
            min_hold_time_minutes: 0,
            max_loss_per_trade_pct: dec!(-0.05),
        };
        assert!(config.validate().is_ok());
        assert_eq!(config.max_position_size_pct, dec!(0.1));
        assert_eq!(config.consecutive_loss_limit, 3);
    }

    #[test]
    fn test_invalid_max_position_size() {
        use rust_decimal_macros::dec;
        let config = RiskConfig {
            max_position_size_pct: dec!(1.5),
            ..RiskConfig::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RiskConfigError::InvalidPercentage {
                field: "max_position_size_pct".to_string(),
                value: dec!(1.5),
            }
        );
    }

    #[test]
    fn test_invalid_negative_percentage() {
        use rust_decimal_macros::dec;
        let config = RiskConfig {
            max_sector_exposure_pct: dec!(-0.1),
            ..RiskConfig::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RiskConfigError::InvalidPercentage {
                field: "max_sector_exposure_pct".to_string(),
                value: dec!(-0.1),
            }
        );
    }

    #[test]
    fn test_invalid_consecutive_loss_limit() {
        let config = RiskConfig {
            consecutive_loss_limit: 0,
            ..RiskConfig::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RiskConfigError::InvalidLimit {
                field: "consecutive_loss_limit".to_string(),
                value: 0,
            }
        );
    }

    #[test]
    fn test_invalid_ttl() {
        let config = RiskConfig {
            pending_order_ttl_ms: Some(-100),
            ..RiskConfig::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RiskConfigError::InvalidTtl {
                field: "pending_order_ttl_ms".to_string(),
                value: -100,
            }
        );
    }

    #[test]
    fn test_boundary_values() {
        use rust_decimal_macros::dec;
        // Test 0.0 (valid minimum)
        let config_min = RiskConfig {
            max_position_size_pct: dec!(0.0),
            max_sector_exposure_pct: dec!(0.0),
            max_daily_loss_pct: dec!(0.0),
            max_drawdown_pct: dec!(0.0),
            ..RiskConfig::default()
        };
        assert!(config_min.validate().is_ok());

        // Test 1.0 (valid maximum)
        let config_max = RiskConfig {
            max_position_size_pct: dec!(1.0),
            max_sector_exposure_pct: dec!(1.0),
            max_daily_loss_pct: dec!(1.0),
            max_drawdown_pct: dec!(1.0),
            ..RiskConfig::default()
        };
        assert!(config_max.validate().is_ok());
    }

    #[test]
    fn test_default_config() {
        use rust_decimal_macros::dec;
        let config = RiskConfig::default();
        assert_eq!(config.max_position_size_pct, dec!(0.1));
        assert_eq!(config.max_sector_exposure_pct, dec!(0.3));
        assert_eq!(config.max_daily_loss_pct, dec!(0.02));
        assert_eq!(config.max_drawdown_pct, dec!(0.1));
        assert_eq!(config.consecutive_loss_limit, 3);
        assert_eq!(config.pending_order_ttl_ms, None);
    }

    #[test]
    fn test_decimal_conversions() {
        let config = RiskConfig::default();

        let max_pos = config.max_position_size_pct;
        assert_eq!(max_pos, dec!(0.1));

        let max_loss = config.max_daily_loss_pct;
        assert_eq!(max_loss, dec!(0.02));
    }
}
