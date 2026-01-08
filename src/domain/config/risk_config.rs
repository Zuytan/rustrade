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
use thiserror::Error;

/// Error type for RiskConfig validation
#[derive(Debug, Error, PartialEq)]
pub enum RiskConfigError {
    #[error("Invalid percentage: {field} = {value}. Must be between 0.0 and 1.0")]
    InvalidPercentage { field: String, value: f64 },
    
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
///
/// let config = RiskConfig::new(
///     0.1,  // max_position_size_pct
///     0.3,  // max_sector_exposure_pct
///     0.02, // max_daily_loss_pct
///     0.1,  // max_drawdown_pct
///     3,    // consecutive_loss_limit
///     Some(5000), // pending_order_ttl_ms
/// ).expect("Valid config");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RiskConfig {
    /// Maximum position size as percentage of portfolio (e.g., 0.1 = 10%)
    pub max_position_size_pct: f64,
    
    /// Maximum sector exposure as percentage of portfolio (e.g., 0.3 = 30%)
    pub max_sector_exposure_pct: f64,
    
    /// Maximum daily loss as percentage of portfolio (e.g., 0.02 = 2%)
    pub max_daily_loss_pct: f64,
    
    /// Maximum drawdown from high-water mark as percentage (e.g., 0.1 = 10%)
    pub max_drawdown_pct: f64,
    
    /// Maximum consecutive losses before halting trading
    pub consecutive_loss_limit: usize,
    
    /// Time-to-live for pending orders in milliseconds (None = no expiration)
    pub pending_order_ttl_ms: Option<i64>,
}

impl RiskConfig {
    /// Create a new RiskConfig with validation
    ///
    /// # Errors
    ///
    /// Returns `RiskConfigError` if any parameter violates invariants
    pub fn new(
        max_position_size_pct: f64,
        max_sector_exposure_pct: f64,
        max_daily_loss_pct: f64,
        max_drawdown_pct: f64,
        consecutive_loss_limit: usize,
        pending_order_ttl_ms: Option<i64>,
    ) -> Result<Self, RiskConfigError> {
        let config = Self {
            max_position_size_pct,
            max_sector_exposure_pct,
            max_daily_loss_pct,
            max_drawdown_pct,
            consecutive_loss_limit,
            pending_order_ttl_ms,
        };
        
        config.validate()?;
        Ok(config)
    }
    
    /// Validate all invariants
    fn validate(&self) -> Result<(), RiskConfigError> {
        // Validate percentages
        self.validate_percentage("max_position_size_pct", self.max_position_size_pct)?;
        self.validate_percentage("max_sector_exposure_pct", self.max_sector_exposure_pct)?;
        self.validate_percentage("max_daily_loss_pct", self.max_daily_loss_pct)?;
        self.validate_percentage("max_drawdown_pct", self.max_drawdown_pct)?;
        
        // Validate consecutive loss limit
        if self.consecutive_loss_limit == 0 {
            return Err(RiskConfigError::InvalidLimit {
                field: "consecutive_loss_limit".to_string(),
                value: self.consecutive_loss_limit,
            });
        }
        
        // Validate TTL if present
        if let Some(ttl) = self.pending_order_ttl_ms && ttl <= 0 {
            return Err(RiskConfigError::InvalidTtl {
                field: "pending_order_ttl_ms".to_string(),
                value: ttl,
            });
        }
        
        Ok(())
    }
    
    /// Validate a percentage field is in range [0.0, 1.0]
    fn validate_percentage(&self, field: &str, value: f64) -> Result<(), RiskConfigError> {
        if !(0.0..=1.0).contains(&value) {
            return Err(RiskConfigError::InvalidPercentage {
                field: field.to_string(),
                value,
            });
        }
        Ok(())
    }
    
    /// Convert max_position_size_pct to Decimal for calculations
    pub fn max_position_size_decimal(&self) -> Decimal {
        Decimal::try_from(self.max_position_size_pct).unwrap_or(Decimal::ZERO)
    }
    
    /// Convert max_daily_loss_pct to Decimal for calculations
    pub fn max_daily_loss_decimal(&self) -> Decimal {
        Decimal::try_from(self.max_daily_loss_pct).unwrap_or(Decimal::ZERO)
    }
    
    /// Convert max_drawdown_pct to Decimal for calculations
    pub fn max_drawdown_decimal(&self) -> Decimal {
        Decimal::try_from(self.max_drawdown_pct).unwrap_or(Decimal::ZERO)
    }
}

impl Default for RiskConfig {
    /// Conservative default risk parameters
    fn default() -> Self {
        Self {
            max_position_size_pct: 0.1,  // 10%
            max_sector_exposure_pct: 0.3, // 30%
            max_daily_loss_pct: 0.02,     // 2%
            max_drawdown_pct: 0.1,        // 10%
            consecutive_loss_limit: 3,
            pending_order_ttl_ms: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let config = RiskConfig::new(0.1, 0.3, 0.02, 0.1, 3, Some(5000));
        assert!(config.is_ok());
        
        let config = config.unwrap();
        assert_eq!(config.max_position_size_pct, 0.1);
        assert_eq!(config.consecutive_loss_limit, 3);
    }

    #[test]
    fn test_invalid_max_position_size() {
        let result = RiskConfig::new(1.5, 0.3, 0.02, 0.1, 3, None);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RiskConfigError::InvalidPercentage {
                field: "max_position_size_pct".to_string(),
                value: 1.5,
            }
        );
    }

    #[test]
    fn test_invalid_negative_percentage() {
        let result = RiskConfig::new(0.1, -0.1, 0.02, 0.1, 3, None);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RiskConfigError::InvalidPercentage {
                field: "max_sector_exposure_pct".to_string(),
                value: -0.1,
            }
        );
    }

    #[test]
    fn test_invalid_consecutive_loss_limit() {
        let result = RiskConfig::new(0.1, 0.3, 0.02, 0.1, 0, None);
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
        let result = RiskConfig::new(0.1, 0.3, 0.02, 0.1, 3, Some(-100));
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
        // Test 0.0 (valid minimum)
        let config = RiskConfig::new(0.0, 0.0, 0.0, 0.0, 1, None);
        assert!(config.is_ok());
        
        // Test 1.0 (valid maximum)
        let config = RiskConfig::new(1.0, 1.0, 1.0, 1.0, 1, Some(1));
        assert!(config.is_ok());
    }

    #[test]
    fn test_default_config() {
        let config = RiskConfig::default();
        assert_eq!(config.max_position_size_pct, 0.1);
        assert_eq!(config.max_sector_exposure_pct, 0.3);
        assert_eq!(config.max_daily_loss_pct, 0.02);
        assert_eq!(config.max_drawdown_pct, 0.1);
        assert_eq!(config.consecutive_loss_limit, 3);
        assert_eq!(config.pending_order_ttl_ms, None);
    }

    #[test]
    fn test_decimal_conversions() {
        let config = RiskConfig::default();
        
        let max_pos = config.max_position_size_decimal();
        assert_eq!(max_pos, Decimal::try_from(0.1).unwrap());
        
        let max_loss = config.max_daily_loss_decimal();
        assert_eq!(max_loss, Decimal::try_from(0.02).unwrap());
    }
}
