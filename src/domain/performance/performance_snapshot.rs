use crate::domain::market::market_regime::MarketRegimeType;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Snapshot of performance at a specific point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSnapshot {
    pub id: Option<i64>,
    pub symbol: String,
    pub timestamp: DateTime<Utc>,
    pub equity: Decimal,
    pub drawdown_pct: Decimal,
    pub sharpe_rolling_30d: f64,
    pub win_rate_rolling_30d: f64,
    pub regime: MarketRegimeType,
}

impl PerformanceSnapshot {
    pub fn new(
        symbol: String,
        equity: Decimal,
        drawdown_pct: Decimal,
        sharpe_rolling_30d: f64,
        win_rate_rolling_30d: f64,
        regime: MarketRegimeType,
    ) -> Self {
        Self {
            id: None,
            symbol,
            timestamp: Utc::now(),
            equity,
            drawdown_pct,
            sharpe_rolling_30d,
            win_rate_rolling_30d,
            regime,
        }
    }
}
