//! Optimal parameters discovered through backtesting optimization.
//!
//! This module provides domain types for storing and retrieving optimized
//! strategy parameters for each risk profile (Conservative/Balanced/Aggressive)
//! and asset type (Stock/Crypto).

use super::risk_appetite::RiskProfile;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Asset type for differentiated optimization parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AssetType {
    #[default]
    Stock,
    Crypto,
}

impl fmt::Display for AssetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetType::Stock => write!(f, "Stock"),
            AssetType::Crypto => write!(f, "Crypto"),
        }
    }
}

impl FromStr for AssetType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stock" | "stocks" => Ok(AssetType::Stock),
            "crypto" | "cryptocurrency" => Ok(AssetType::Crypto),
            _ => Err(format!("Unknown asset type: {}", s)),
        }
    }
}

/// Optimal strategy parameters discovered through backtesting.
///
/// These parameters represent the best-performing configuration found
/// by the grid search optimizer for a specific risk profile and asset type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalParameters {
    /// Asset type (Stock or Crypto)
    pub asset_type: AssetType,
    /// Risk profile these parameters are optimized for
    pub risk_profile: RiskProfile,

    // Strategy parameters
    /// Fast SMA period for trend detection
    pub fast_sma_period: usize,
    /// Slow SMA period for trend detection
    pub slow_sma_period: usize,
    /// RSI threshold for entry signals
    pub rsi_threshold: Decimal,
    /// ATR multiplier for trailing stop
    pub trailing_stop_atr_multiplier: Decimal,
    /// Threshold for trend-price divergence filter
    pub trend_divergence_threshold: Decimal,
    /// Cooldown between orders in seconds
    pub order_cooldown_seconds: u64,

    // Optimization metadata
    /// When the optimization was run
    pub optimization_date: DateTime<Utc>,
    /// Symbol used for optimization
    pub symbol_used: String,
    /// Sharpe ratio achieved during optimization
    pub sharpe_ratio: Decimal,
    /// Total return percentage achieved
    pub total_return: Decimal,
    /// Maximum drawdown percentage
    pub max_drawdown: Decimal,
    /// Win rate percentage
    pub win_rate: Decimal,
    /// Total number of trades in optimization
    pub total_trades: usize,
}

impl OptimalParameters {
    /// Creates a new OptimalParameters instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        asset_type: AssetType,
        risk_profile: RiskProfile,
        fast_sma_period: usize,
        slow_sma_period: usize,
        rsi_threshold: Decimal,
        trailing_stop_atr_multiplier: Decimal,
        trend_divergence_threshold: Decimal,
        order_cooldown_seconds: u64,
        symbol_used: String,
        sharpe_ratio: Decimal,
        total_return: Decimal,
        max_drawdown: Decimal,
        win_rate: Decimal,
        total_trades: usize,
    ) -> Self {
        Self {
            asset_type,
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

/// Collection of optimal parameters for all risk profiles and asset types.
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

    /// Adds or updates parameters for a risk profile and asset type combination.
    pub fn upsert(&mut self, params: OptimalParameters) {
        // Remove existing entry for this profile + asset type combination
        self.parameters.retain(|p| {
            !(p.risk_profile == params.risk_profile && p.asset_type == params.asset_type)
        });
        self.parameters.push(params);
    }

    /// Gets parameters for a specific risk profile (defaults to Stock).
    pub fn get(&self, profile: RiskProfile) -> Option<&OptimalParameters> {
        self.get_by_type(AssetType::Stock, profile)
    }

    /// Gets parameters for a specific asset type and risk profile.
    pub fn get_by_type(
        &self,
        asset_type: AssetType,
        profile: RiskProfile,
    ) -> Option<&OptimalParameters> {
        self.parameters
            .iter()
            .find(|p| p.risk_profile == profile && p.asset_type == asset_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_optimal_parameters_creation() {
        let params = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Balanced,
            20,
            60,
            dec!(65.0),
            dec!(3.0),
            dec!(0.005),
            300,
            "AAPL".to_string(),
            dec!(1.5),
            dec!(15.0),
            dec!(5.0),
            dec!(60.0),
            50,
        );

        assert_eq!(params.asset_type, AssetType::Stock);
        assert_eq!(params.risk_profile, RiskProfile::Balanced);
        assert_eq!(params.fast_sma_period, 20);
        assert_eq!(params.slow_sma_period, 60);
        assert_eq!(params.rsi_threshold, dec!(65.0));
        assert_eq!(params.symbol_used, "AAPL");
    }

    #[test]
    fn test_optimal_parameters_set_upsert() {
        let mut set = OptimalParametersSet::new();

        let params1 = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Conservative,
            10,
            50,
            dec!(60.0),
            dec!(2.0),
            dec!(0.003),
            600,
            "TSLA".to_string(),
            dec!(1.2),
            dec!(10.0),
            dec!(3.0),
            dec!(55.0),
            30,
        );

        let params2 = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Conservative,
            15,
            55,
            dec!(62.0),
            dec!(2.5),
            dec!(0.004),
            500,
            "AAPL".to_string(),
            dec!(1.8),
            dec!(18.0),
            dec!(4.0),
            dec!(62.0),
            40,
        );

        set.upsert(params1);
        assert_eq!(set.parameters.len(), 1);

        // Upsert should replace existing for same asset_type + profile
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

        let conservative_stock = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Conservative,
            10,
            50,
            dec!(60.0),
            dec!(2.0),
            dec!(0.003),
            600,
            "TSLA".to_string(),
            dec!(1.2),
            dec!(10.0),
            dec!(3.0),
            dec!(55.0),
            30,
        );

        let aggressive_crypto = OptimalParameters::new(
            AssetType::Crypto,
            RiskProfile::Aggressive,
            30,
            100,
            dec!(70.0),
            dec!(4.0),
            dec!(0.01),
            0,
            "BTCUSD".to_string(),
            dec!(2.0),
            dec!(25.0),
            dec!(8.0),
            dec!(65.0),
            80,
        );

        set.upsert(conservative_stock);
        set.upsert(aggressive_crypto);

        // Default get uses Stock
        assert!(set.get(RiskProfile::Conservative).is_some());
        assert!(set.get(RiskProfile::Aggressive).is_none()); // Aggressive is Crypto

        // Specific get_by_type
        assert!(
            set.get_by_type(AssetType::Stock, RiskProfile::Conservative)
                .is_some()
        );
        assert!(
            set.get_by_type(AssetType::Crypto, RiskProfile::Aggressive)
                .is_some()
        );
        assert!(
            set.get_by_type(AssetType::Stock, RiskProfile::Aggressive)
                .is_none()
        );
    }
}
