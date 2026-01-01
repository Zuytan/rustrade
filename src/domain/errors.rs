use thiserror::Error;
use rust_decimal::Decimal;

/// Errors related to trading operations and portfolio management
#[derive(Debug, Error)]
pub enum TradingError {
    #[error("Insufficient funds: need ${need}, available ${available}")]
    InsufficientFunds { need: Decimal, available: Decimal },
    
    #[error("Position not found: {symbol}")]
    PositionNotFound { symbol: String },
    
    #[error("Invalid order: {reason}")]
    InvalidOrder { reason: String },
    
    #[error("Order execution failed: {reason}")]
    ExecutionFailed { reason: String },
}

/// Errors related to risk management violations
#[derive(Debug, Error)]
pub enum RiskViolation {
    #[error("Position size limit exceeded for {symbol}: {current_pct:.2}% > {max_pct:.2}%")]
    PositionSizeLimit {
        symbol: String,
        current_pct: f64,
        max_pct: f64,
    },
    
    #[error("Daily loss limit breached: {loss_pct:.2}% > {limit_pct:.2}%")]
    DailyLossLimit { loss_pct: f64, limit_pct: f64 },
    
    #[error("Maximum drawdown exceeded: {drawdown_pct:.2}% > {max_pct:.2}%")]
    MaxDrawdown { drawdown_pct: f64, max_pct: f64 },
    
    #[error("Sector exposure limit for {sector}: {current_pct:.2}% > {max_pct:.2}%")]
    SectorExposureLimit {
        sector: String,
        current_pct: f64,
        max_pct: f64,
    },
    
    #[error("PDT protection: {day_trades} day trades with equity ${equity} < $25,000")]
    PdtProtection { day_trades: u64, equity: Decimal },
    
    #[error("Consecutive loss limit reached: {count} losses")]
    ConsecutiveLossLimit { count: usize },
}

/// Errors related to market data and connectivity
#[derive(Debug, Error)]
pub enum MarketDataError {
    #[error("Connection lost: {reason}")]
    ConnectionLost { reason: String },
    
    #[error("Invalid market data for {symbol}: {reason}")]
    InvalidData { symbol: String, reason: String },
    
    #[error("Service timeout after {duration_ms}ms")]
    Timeout { duration_ms: u64 },
    
    #[error("Rate limit exceeded: retry after {retry_after_secs}s")]
    RateLimitExceeded { retry_after_secs: u64 },
}

/// Errors related to portfolio state management
#[derive(Debug, Error)]
pub enum PortfolioError {
    #[error("Portfolio snapshot is stale: age {age_ms}ms > limit {limit_ms}ms")]
    StaleSnapshot { age_ms: u64, limit_ms: u64 },
    
    #[error("Version conflict: expected v{expected}, got v{actual}")]
    VersionConflict { expected: u64, actual: u64 },
    
    #[error("Exposure reservation failed for {symbol}: {reason}")]
    ReservationFailed { symbol: String, reason: String },
    
    #[error("Failed to refresh portfolio state: {reason}")]
    RefreshFailed { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_violation_formatting() {
        let violation = RiskViolation::PositionSizeLimit {
            symbol: "AAPL".to_string(),
            current_pct: 15.5,
            max_pct: 10.0,
        };
        
        let msg = violation.to_string();
        assert!(msg.contains("AAPL"));
        assert!(msg.contains("15.50%"));
        assert!(msg.contains("10.00%"));
    }

    #[test]
    fn test_portfolio_error_formatting() {
        let error = PortfolioError::StaleSnapshot {
            age_ms: 7000,
            limit_ms: 5000,
        };
        
        let msg = error.to_string();
        assert!(msg.contains("7000"));
        assert!(msg.contains("5000"));
    }
}
