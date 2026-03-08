use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::risk::optimal_parameters::{AssetType, OptimalParameters};
use crate::domain::risk::risk_appetite::{RiskAppetite, RiskProfile};
use crate::domain::trading::types::Candle;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// Single optimization result. In walk-forward mode, sharpe_ratio is OOS and in_sample_sharpe is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub params: AnalystConfig,
    pub sharpe_ratio: Decimal,
    pub total_return: Decimal,
    pub max_drawdown: Decimal,
    pub win_rate: Decimal,
    pub total_trades: usize,
    pub objective_score: Decimal,
    pub alpha: Decimal,
    pub beta: Decimal,
    /// When set, optimization used train/test split; sharpe_ratio is OOS, this is in-sample.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_sample_sharpe: Option<Decimal>,
    /// Risk score (1-9) when optimization was run with --risk-score. Enables loading best params per risk in benchmark.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_score: Option<u8>,
}

impl OptimizationResult {
    /// Calculate a weighted objective score for ranking configurations
    /// Higher is better
    pub fn calculate_objective_score(&mut self) {
        // Multi-criteria optimization (Sharpe + MaxDD + Stability)

        // 1. Return/Risk components
        let sharpe_component = self.sharpe_ratio * dec!(0.5);
        let return_component = (self.total_return / dec!(100.0)) * dec!(0.2);
        let win_rate_component = (self.win_rate / dec!(100.0)) * dec!(0.1);

        // 2. Drawdown component (Non-linear penalty for drawdowns)
        let dd_ratio = self.max_drawdown.abs() / dec!(100.0);
        let dd_penalty = dd_ratio * dd_ratio * dec!(2.0); // Quadratic penalty

        // 3. Stability Penalty (penalize if total trades is less than 30)
        let trades_dec = Decimal::from(self.total_trades);
        let stability_penalty = if self.total_trades < 30 {
            let penalty_factor = (dec!(30.0) - trades_dec) / dec!(30.0);
            penalty_factor * dec!(2.0)
        } else {
            Decimal::ZERO
        };

        self.objective_score = sharpe_component + return_component + win_rate_component
            - dd_penalty
            - stability_penalty;
    }

    /// Build OptimalParameters for persistence (e.g. ~/.rustrade/optimal_parameters.json) from this result.
    /// Call with the risk_score and asset_type used for the run, and the symbol optimized.
    pub fn to_optimal_parameters(
        &self,
        risk_score: u8,
        asset_type: AssetType,
        symbol_used: String,
    ) -> OptimalParameters {
        let profile = RiskAppetite::new(risk_score)
            .map(|r| r.profile())
            .unwrap_or(RiskProfile::Balanced);
        OptimalParameters::new(
            asset_type,
            profile,
            self.params.strategy.fast_sma_period,
            self.params.strategy.slow_sma_period,
            self.params.strategy.rsi_threshold,
            self.params.strategy.trailing_stop_atr_multiplier,
            self.params.strategy.trend_divergence_threshold,
            self.params.risk.order_cooldown_seconds,
            symbol_used,
            self.sharpe_ratio,
            self.total_return,
            self.max_drawdown,
            self.win_rate,
            self.total_trades,
        )
        .with_risk_score(risk_score)
    }

    /// Build OptimalParameters for a given profile without risk_score (used when saving "for all" without --risk-score).
    pub fn to_optimal_parameters_for_profile(
        &self,
        profile: RiskProfile,
        asset_type: AssetType,
        symbol_used: String,
    ) -> OptimalParameters {
        OptimalParameters::new(
            asset_type,
            profile,
            self.params.strategy.fast_sma_period,
            self.params.strategy.slow_sma_period,
            self.params.strategy.rsi_threshold,
            self.params.strategy.trailing_stop_atr_multiplier,
            self.params.strategy.trend_divergence_threshold,
            self.params.risk.order_cooldown_seconds,
            symbol_used,
            self.sharpe_ratio,
            self.total_return,
            self.max_drawdown,
            self.win_rate,
            self.total_trades,
        )
    }
}

/// Pre-fetched bars for walk-forward optimization (avoids repeated API calls).
pub struct PrefetchedBars {
    pub train_bars: Vec<Candle>,
    pub train_start: DateTime<Utc>,
    pub train_end: DateTime<Utc>,
    pub test_bars: Vec<Candle>,
    pub test_start: DateTime<Utc>,
    pub test_end: DateTime<Utc>,
    pub spy_train: Vec<Candle>,
    pub spy_test: Vec<Candle>,
}

/// Pre-fetched bars for single-period optimization (one backtest over full range).
pub struct SinglePeriodBars {
    pub bars: Vec<Candle>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub spy_bars: Vec<Candle>,
}
