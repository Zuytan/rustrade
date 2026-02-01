use crate::domain::market::market_regime::MarketRegimeType;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Record of an optimization run and the resulting parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationHistory {
    pub id: Option<i64>,
    pub symbol: String,
    pub timestamp: DateTime<Utc>,
    pub parameters_json: String,
    pub performance_metrics_json: String,
    pub market_regime: MarketRegimeType,
    pub sharpe_ratio: Decimal,
    pub total_return: Decimal,
    pub win_rate: Decimal,
    pub is_active: bool,
}

impl OptimizationHistory {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        symbol: String,
        parameters_json: String,
        performance_metrics_json: String,
        market_regime: MarketRegimeType,
        sharpe_ratio: Decimal,
        total_return: Decimal,
        win_rate: Decimal,
    ) -> Self {
        Self {
            id: None,
            symbol,
            timestamp: Utc::now(),
            parameters_json,
            performance_metrics_json,
            market_regime,
            sharpe_ratio,
            total_return,
            win_rate,
            is_active: true, // New optimizations are active by default
        }
    }
}
