use crate::domain::ports::SectorProvider;
use crate::domain::risk::filters::correlation_filter::CorrelationFilterConfig;
use crate::domain::risk::volatility_manager::VolatilityConfig;
use std::sync::Arc;

/// Error type for RiskManager configuration validation
#[derive(Debug, thiserror::Error)]
pub enum RiskConfigError {
    #[error("Invalid RiskConfig: {0}")]
    ValidationError(String),
}

/// Risk management configuration
#[derive(Clone)]
pub struct RiskConfig {
    pub max_position_size_pct: f64, // Max % of equity per position (e.g., 0.25 = 25%)
    pub max_daily_loss_pct: f64,    // Max % loss per day (e.g., 0.02 = 2%)
    pub max_drawdown_pct: f64,      // Max % drawdown from high water mark (e.g., 0.10 = 10%)
    pub consecutive_loss_limit: usize, // Max consecutive losing trades before halt
    pub valuation_interval_seconds: u64, // Interval for portfolio valuation check
    pub max_sector_exposure_pct: f64, // Max exposure per sector
    pub sector_provider: Option<Arc<dyn SectorProvider>>,
    pub allow_pdt_risk: bool, // If true, allows opening orders even if PDT saturated (Risky!)
    pub pending_order_ttl_ms: Option<i64>, // TTL for pending orders filled but not synced
    pub correlation_config: CorrelationFilterConfig,
    pub volatility_config: VolatilityConfig, // Added
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
            .finish()
    }
}

impl RiskConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_position_size_pct <= 0.0 || self.max_position_size_pct > 1.0 {
            return Err(format!(
                "Invalid max_position_size_pct: {}",
                self.max_position_size_pct
            ));
        }
        if self.max_daily_loss_pct <= 0.0 || self.max_daily_loss_pct > 0.5 {
            return Err(format!(
                "Invalid max_daily_loss_pct: {}",
                self.max_daily_loss_pct
            ));
        }
        if self.max_drawdown_pct <= 0.0 || self.max_drawdown_pct > 1.0 {
            return Err(format!(
                "Invalid max_drawdown_pct: {}",
                self.max_drawdown_pct
            ));
        }
        if self.consecutive_loss_limit == 0 {
            return Err("consecutive_loss_limit must be > 0".to_string());
        }
        if self.max_sector_exposure_pct <= 0.0 || self.max_sector_exposure_pct > 1.0 {
            return Err(format!(
                "Invalid max_sector_exposure_pct: {}",
                self.max_sector_exposure_pct
            ));
        }
        Ok(())
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_size_pct: 0.10, // Reduced from 0.25 for safety
            max_daily_loss_pct: 0.02,    // 2%
            max_drawdown_pct: 0.05,      // Reduced from 0.10 for safety
            consecutive_loss_limit: 3,
            valuation_interval_seconds: 60,
            max_sector_exposure_pct: 0.20, // Reduced from 0.30

            sector_provider: None,
            allow_pdt_risk: false,
            pending_order_ttl_ms: None, // Default 5 mins
            correlation_config: CorrelationFilterConfig::default(),
            volatility_config: VolatilityConfig::default(),
        }
    }
}
