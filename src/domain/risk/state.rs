use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Persistent state of the Risk Manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskState {
    /// Unique identifier (usually "global")
    pub id: String,

    /// Equity at the start of the current session
    pub session_start_equity: Decimal,

    /// Equity at the start of the current trading day
    pub daily_start_equity: Decimal,

    /// Highest equity reached (High Water Mark)
    pub equity_high_water_mark: Decimal,

    /// Number of consecutive losing trades
    pub consecutive_losses: usize,

    /// Date of the last reference update (for daily reset)
    /// Date of the last reference update (for daily reset)
    pub reference_date: NaiveDate,

    /// Timestamp of last state update
    pub updated_at: i64,

    /// Flag indicating if daily drawdown has been reset
    pub daily_drawdown_reset: bool,
}

impl Default for RiskState {
    fn default() -> Self {
        Self {
            id: "global".to_string(),
            session_start_equity: Decimal::ZERO,
            daily_start_equity: Decimal::ZERO,
            equity_high_water_mark: Decimal::ZERO,
            consecutive_losses: 0,
            reference_date: chrono::Utc::now().date_naive(),
            updated_at: chrono::Utc::now().timestamp(),
            daily_drawdown_reset: false,
        }
    }
}
