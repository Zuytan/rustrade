use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerReason {
    PoorPerformance, // Sharpe/WinRate drop
    RegimeChange,    // Market shifted (e.g. Trending -> Range)
    DrawdownLimit,   // Exceeded max drawdown
    Scheduled,       // Regular daily check
    Manual,          // User forced
}

impl fmt::Display for TriggerReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TriggerReason::PoorPerformance => write!(f, "Poor Performance"),
            TriggerReason::RegimeChange => write!(f, "Regime Change"),
            TriggerReason::DrawdownLimit => write!(f, "Drawdown Limit"),
            TriggerReason::Scheduled => write!(f, "Scheduled"),
            TriggerReason::Manual => write!(f, "Manual"),
        }
    }
}

/// Event recording why a re-optimization was triggered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReoptimizationTrigger {
    pub id: Option<i64>,
    pub symbol: String,
    pub timestamp: DateTime<Utc>,
    pub trigger_reason: TriggerReason,
    pub status: String, // pending, running, completed, failed
    pub result_json: Option<String>,
}

impl ReoptimizationTrigger {
    pub fn new(symbol: String, reason: TriggerReason) -> Self {
        Self {
            id: None,
            symbol,
            timestamp: Utc::now(),
            trigger_reason: reason,
            status: "pending".to_string(),
            result_json: None,
        }
    }
}
