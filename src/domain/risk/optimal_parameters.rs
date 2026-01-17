//! Optimal parameters discovered through backtesting optimization.
//!
//! This module provides domain types for storing and retrieving optimized
//! strategy parameters for each risk profile (Conservative/Balanced/Aggressive).

use super::risk_appetite::RiskProfile;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Optimal strategy parameters discovered through backtesting.
///
/// These parameters represent the best-performing configuration found
/// by the grid search optimizer for a specific risk profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalParameters {
    /// Risk profile these parameters are optimized for
    pub risk_profile: RiskProfile,

    // Strategy parameters
    /// Fast SMA period for trend detection
    pub fast_sma_period: usize,
    /// Slow SMA period for trend detection
    pub slow_sma_period: usize,
    /// RSI threshold for entry signals
    pub rsi_threshold: f64,
    /// ATR multiplier for trailing stop
    pub trailing_stop_atr_multiplier: f64,
    /// Threshold for trend-price divergence filter
    pub trend_divergence_threshold: f64,
    /// Cooldown between orders in seconds
    pub order_cooldown_seconds: u64,

    // Optimization metadata
    /// When the optimization was run
    pub optimization_date: DateTime<Utc>,
    /// Symbol used for optimization
    pub symbol_used: String,
    /// Sharpe ratio achieved during optimization
    pub sharpe_ratio: f64,
    /// Total return percentage achieved
    pub total_return: f64,
    /// Maximum drawdown percentage
    pub max_drawdown: f64,
    /// Win rate percentage
    pub win_rate: f64,
    /// Total number of trades in optimization
    pub total_trades: usize,
}

impl OptimalParameters {
    /// Creates a new OptimalParameters instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        risk_profile: RiskProfile,
        fast_sma_period: usize,
        slow_sma_period: usize,
        rsi_threshold: f64,
        trailing_stop_atr_multiplier: f64,
        trend_divergence_threshold: f64,
        order_cooldown_seconds: u64,
        symbol_used: String,
        sharpe_ratio: f64,
        total_return: f64,
        max_drawdown: f64,
        win_rate: f64,
        total_trades: usize,
    ) -> Self {
        Self {
            risk_profile,
            fast_sma_period,
            slow_sma_period,
            rsi_threshold,
            trailing_stop_atr_multiplier,
            trend_divergence_threshold,
            order_cooldown_seconds,
            optimization_date: Utc::now(),
            symbol_used,
            sharpe_ratio,
            total_return,
            max_drawdown,
            win_rate,
            total_trades,
        }
    }
}

/// Collection of optimal parameters for all risk profiles.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OptimalParametersSet {
    pub parameters: Vec<OptimalParameters>,
}

impl OptimalParametersSet {
    /// Creates a new empty set.
    pub fn new() -> Self {
        Self {
            parameters: Vec::new(),
        }
    }

    /// Adds or updates parameters for a risk profile.
    pub fn upsert(&mut self, params: OptimalParameters) {
        // Remove existing entry for this profile if present
        self.parameters
            .retain(|p| p.risk_profile != params.risk_profile);
        self.parameters.push(params);
    }

    /// Gets parameters for a specific risk profile.
    pub fn get(&self, profile: RiskProfile) -> Option<&OptimalParameters> {
        self.parameters.iter().find(|p| p.risk_profile == profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimal_parameters_creation() {
        let params = OptimalParameters::new(
            RiskProfile::Balanced,
            20,
            60,
            65.0,
            3.0,
            0.005,
            300,
            "AAPL".to_string(),
            1.5,
            15.0,
            5.0,
            60.0,
            50,
        );

        assert_eq!(params.risk_profile, RiskProfile::Balanced);
        assert_eq!(params.fast_sma_period, 20);
        assert_eq!(params.slow_sma_period, 60);
        assert_eq!(params.rsi_threshold, 65.0);
        assert_eq!(params.symbol_used, "AAPL");
    }

    #[test]
    fn test_optimal_parameters_set_upsert() {
        let mut set = OptimalParametersSet::new();

        let params1 = OptimalParameters::new(
            RiskProfile::Conservative,
            10,
            50,
            60.0,
            2.0,
            0.003,
            600,
            "TSLA".to_string(),
            1.2,
            10.0,
            3.0,
            55.0,
            30,
        );

        let params2 = OptimalParameters::new(
            RiskProfile::Conservative,
            15,
            55,
            62.0,
            2.5,
            0.004,
            500,
            "AAPL".to_string(),
            1.8,
            18.0,
            4.0,
            62.0,
            40,
        );

        set.upsert(params1);
        assert_eq!(set.parameters.len(), 1);

        // Upsert should replace existing
        set.upsert(params2);
        assert_eq!(set.parameters.len(), 1);
        assert_eq!(
            set.get(RiskProfile::Conservative).unwrap().fast_sma_period,
            15
        );
    }

    #[test]
    fn test_optimal_parameters_set_get() {
        let mut set = OptimalParametersSet::new();

        let conservative = OptimalParameters::new(
            RiskProfile::Conservative,
            10,
            50,
            60.0,
            2.0,
            0.003,
            600,
            "TSLA".to_string(),
            1.2,
            10.0,
            3.0,
            55.0,
            30,
        );

        let aggressive = OptimalParameters::new(
            RiskProfile::Aggressive,
            30,
            100,
            70.0,
            4.0,
            0.01,
            0,
            "NVDA".to_string(),
            2.0,
            25.0,
            8.0,
            65.0,
            80,
        );

        set.upsert(conservative);
        set.upsert(aggressive);

        assert!(set.get(RiskProfile::Conservative).is_some());
        assert!(set.get(RiskProfile::Aggressive).is_some());
        assert!(set.get(RiskProfile::Balanced).is_none());
    }
}
