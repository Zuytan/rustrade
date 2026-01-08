use crate::application::market_data::spread_cache::SpreadCache;
use crate::domain::trading::types::TradeProposal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use crate::domain::trading::fee_model::FeeModel;
use std::sync::Arc;

/// Detailed breakdown of transaction costs for a trade
#[derive(Debug, Clone)]
pub struct TradeCost {
    /// Per-share commission fee
    pub commission: Decimal,
    /// Estimated slippage cost (price impact)
    pub estimated_slippage: Decimal,
    /// Estimated bid-ask spread cost
    pub spread_cost: Decimal,
    /// Total cost = commission + slippage + spread
    pub total_cost: Decimal,
}

/// Service for calculating transaction costs and validating trade profitability
///
/// Implements the "Cost-Aware Trading" feature to prevent executing trades
/// that are unprofitable after accounting for commissions, slippage, and spreads.
///
/// # Example
/// ```
/// use rustrade::application::monitoring::cost_evaluator::CostEvaluator;
/// use rustrade::domain::trading::types::{TradeProposal, OrderSide, OrderType};
/// use rust_decimal::Decimal;
///
/// let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
/// let proposal = TradeProposal {
///     symbol: "AAPL".to_string(),
///     side: OrderSide::Buy,
///     price: Decimal::from(100),
///     quantity: Decimal::from(10),
///     order_type: OrderType::Market,
///     reason: "Test".to_string(),
///     timestamp: 0,
/// };
/// let costs = evaluator.evaluate(&proposal);
/// let expected_profit = Decimal::from(5);
///
/// if costs.total_cost > expected_profit {
///     // Reject trade - costs exceed expected profit
/// }
/// ```
#[derive(Debug, Clone)]
pub struct CostEvaluator {
    /// Centralized Fee Model (Strategy Pattern)
    fee_model: Arc<dyn FeeModel>,
    /// DEFAULT spread in basis points (fallback when real spread unavailable)
    default_spread_bps: Decimal,
    /// Real-time spread cache (optional - uses default if None or stale)
    spread_cache: Option<Arc<SpreadCache>>,
}

impl CostEvaluator {
    /// Create a new CostEvaluator with specified fee model
    pub fn new(fee_model: Arc<dyn FeeModel>, spread_bps: f64) -> Self {
        Self {
            fee_model,
            default_spread_bps: Decimal::from_f64(spread_bps).unwrap_or(Decimal::ZERO),
            spread_cache: None,
        }
    }

    /// Create CostEvaluator with real-time spread tracking
    pub fn with_spread_cache(
        fee_model: Arc<dyn FeeModel>,
        default_spread_bps: f64,
        spread_cache: Arc<SpreadCache>,
    ) -> Self {
        Self {
            fee_model,
            default_spread_bps: Decimal::from_f64(default_spread_bps).unwrap_or(Decimal::ZERO),
            spread_cache: Some(spread_cache),
        }
    }

    /// Calculate total transaction costs for a trade proposal
    ///
    /// # Arguments
    /// * `proposal` - The trade proposal to evaluate
    ///
    /// # Returns
    /// TradeCost breakdown with detailed cost components
    pub fn evaluate(&self, proposal: &TradeProposal) -> TradeCost {
        // Delegate to FeeModel
        let trade_costs = self.fee_model.calculate_cost(proposal.quantity, proposal.price, proposal.side);
        let commission = trade_costs.fee;
        let estimated_slippage = trade_costs.slippage_cost;

        // Maximum spread caps to prevent unrealistically wide spreads from low-liquidity periods
        // Crypto altcoins: max 25 bps, Stocks: max 15 bps
        let is_crypto = proposal.symbol.contains('/');
        let max_spread_bps = if is_crypto {
            Decimal::from(25) // 25 bps max for crypto
        } else {
            Decimal::from(15) // 15 bps max for stocks
        };

        // Spread: Use REAL spread from cache if available, otherwise use default
        let raw_spread_bps = if let Some(ref cache) = self.spread_cache {
            if let Some(real_spread_pct) = cache.get_spread_pct(&proposal.symbol) {
                Decimal::from_f64(real_spread_pct * 10000.0).unwrap_or(self.default_spread_bps)
            } else {
                tracing::debug!(
                    "CostEvaluator: No real spread for {}, using DEFAULT {:.2} bps",
                    proposal.symbol,
                    self.default_spread_bps
                );
                self.default_spread_bps
            }
        } else {
            self.default_spread_bps
        };

        // Apply spread cap
        let capped = raw_spread_bps > max_spread_bps;
        let spread_bps = if capped {
            tracing::debug!(
                "💰 CostEvaluator: {} spread {:.2} bps capped to {:.2} bps",
                proposal.symbol,
                raw_spread_bps,
                max_spread_bps
            );
            max_spread_bps
        } else {
            raw_spread_bps
        };

        // HALF-SPREAD cost: A single trade direction only pays half the bid-ask spread
        // When you BUY, you pay ask (mid + half_spread) instead of mid
        // When you SELL, you receive bid (mid - half_spread) instead of mid
        let half_spread_bps = spread_bps / Decimal::from(2);
        let trade_value = proposal.price * proposal.quantity;
        let spread_cost = trade_value * (half_spread_bps / Decimal::from(10000));

        // Total cost is sum of all components
        let total_cost = commission + estimated_slippage + spread_cost;

        tracing::info!(
            "💵 {} Cost: Comm=${:.2}, Slip=${:.2}, Spread=${:.2} ({:.1} bps, half of {:.1}), TOTAL=${:.2}{}",
            proposal.symbol,
            commission.to_f64().unwrap_or(0.0),
            estimated_slippage.to_f64().unwrap_or(0.0),
            spread_cost.to_f64().unwrap_or(0.0),
            half_spread_bps.to_f64().unwrap_or(0.0),
            spread_bps.to_f64().unwrap_or(0.0),
            total_cost.to_f64().unwrap_or(0.0),
            if capped { " [CAPPED]" } else { "" }
        );

        TradeCost {
            commission,
            estimated_slippage,
            spread_cost,
            total_cost,
        }
    }

    /// Check if a trade is profitable after accounting for costs
    ///
    /// A trade is considered profitable if:
    /// expected_profit >= total_cost * min_profit_ratio
    ///
    /// # Arguments
    /// * `proposal` - The trade proposal to evaluate
    /// * `expected_profit` - Expected profit from the trade
    /// * `min_profit_ratio` - Minimum ratio of profit to costs (e.g., 2.0 = profit must be 2x costs)
    ///
    /// # Returns
    /// `true` if the trade meets the minimum profit ratio, `false` otherwise
    ///
    /// # Example
    /// ```
    /// use rustrade::application::monitoring::cost_evaluator::CostEvaluator;
    /// use rustrade::domain::trading::types::{TradeProposal, OrderSide, OrderType};
    /// use rust_decimal::Decimal;
    ///
    /// let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
    /// let proposal = TradeProposal {
    ///     symbol: "AAPL".to_string(),
    ///     side: OrderSide::Buy,
    ///     price: Decimal::from(100),
    ///     quantity: Decimal::from(10),
    ///     order_type: OrderType::Market,
    ///     reason: "Test".to_string(),
    ///     timestamp: 0,
    /// };
    ///
    /// // Trade costs $1.50, expected profit is $5.00, min ratio is 2.0
    /// // Threshold = $1.50 * 2.0 = $3.00
    /// // $5.00 >= $3.00 → Profitable ✅
    /// let is_profitable = evaluator.is_profitable(&proposal, Decimal::from(5), 2.0);
    /// assert!(is_profitable);
    /// ```
    pub fn is_profitable(
        &self,
        proposal: &TradeProposal,
        expected_profit: Decimal,
        min_profit_ratio: f64,
    ) -> bool {
        let costs = self.evaluate(proposal);
        let min_threshold =
            costs.total_cost * Decimal::from_f64(min_profit_ratio).unwrap_or(Decimal::from(2));

        expected_profit >= min_threshold
    }

    /// Calculate expected profit for a proposal based on ATR
    ///
    /// Uses a conservative estimate: ATR * profit_target_multiplier * quantity
    ///
    /// # Arguments
    /// * `proposal` - The trade proposal
    /// * `atr` - Average True Range (volatility measure)
    /// * `profit_target_multiplier` - Multiplier for profit target (e.g., 1.5 = 1.5x ATR)
    ///
    /// # Returns
    /// Expected profit in dollars
    pub fn calculate_expected_profit(
        &self,
        proposal: &TradeProposal,
        atr: f64,
        profit_target_multiplier: f64,
    ) -> Decimal {
        let atr_decimal = Decimal::from_f64(atr).unwrap_or(Decimal::ZERO);
        let multiplier = Decimal::from_f64(profit_target_multiplier).unwrap_or(Decimal::ONE);

        // Expected profit = ATR * multiplier * quantity
        atr_decimal * multiplier * proposal.quantity
    }

    /// Get profit-to-cost ratio for a trade
    ///
    /// # Returns
    /// Ratio of expected profit to total costs (e.g., 2.5 means profit is 2.5x costs)
    /// Returns 0.0 if costs are zero (edge case)
    pub fn get_profit_cost_ratio(&self, proposal: &TradeProposal, expected_profit: Decimal) -> f64 {
        let costs = self.evaluate(proposal);

        if costs.total_cost <= Decimal::ZERO {
            return 0.0;
        }

        let ratio = expected_profit / costs.total_cost;
        ratio.to_f64().unwrap_or(0.0)
    }
}

impl Default for CostEvaluator {
    /// Create CostEvaluator with conservative default parameters
    ///
    /// Defaults:
    /// - Commission: $0.005 per share (typical for discount brokers)
    /// - Slippage: 0.1% of trade value
    /// - Spread: 5 basis points
    fn default() -> Self {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        Self::new(
            Arc::new(ConstantFeeModel::new(Decimal::from_f64(0.005).unwrap(), Decimal::from_f64(0.001).unwrap())),
            5.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::{OrderSide, OrderType};
    use rust_decimal_macros::dec;

    fn create_test_proposal(price: Decimal, quantity: Decimal) -> TradeProposal {
        TradeProposal {
            symbol: "TEST".to_string(),
            side: OrderSide::Buy,
            price,
            quantity,
            order_type: OrderType::Limit,
            reason: "Test trade".to_string(),
            timestamp: 0,
        }
    }

    #[test]
    fn test_cost_evaluation_components() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        let costs = evaluator.evaluate(&proposal);

        // Commission: 10 shares * $0.005 = $0.05
        assert_eq!(costs.commission, dec!(0.05));

        // Slippage: $1000 (trade value) * 0.001 = $1.00
        assert_eq!(costs.estimated_slippage, dec!(1.0));

        // Spread: $1000 * (5/2 / 10000) = $0.25 (HALF-SPREAD: single direction only)
        assert_eq!(costs.spread_cost, dec!(0.25));

        // Total: $0.05 + $1.00 + $0.25 = $1.30
        assert_eq!(costs.total_cost, dec!(1.30));
    }

    #[test]
    fn test_profitability_check_pass() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.55 (from previous test)
        // Min threshold: $1.55 * 2.0 = $3.10
        // Expected profit: $5.00 > $3.10 ✅
        assert!(evaluator.is_profitable(&proposal, dec!(5.0), 2.0));
    }

    #[test]
    fn test_profitability_check_fail() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.55
        // Min threshold: $1.55 * 2.0 = $3.10
        // Expected profit: $2.00 < $3.10 ❌
        assert!(!evaluator.is_profitable(&proposal, dec!(2.0), 2.0));
    }

    #[test]
    fn test_profitability_exact_threshold() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.55
        // Min threshold: $1.55 * 2.0 = $3.10
        // Expected profit: $3.10 = $3.10 ✅ (equal passes)
        assert!(evaluator.is_profitable(&proposal, dec!(3.10), 2.0));
    }

    #[test]
    fn test_expected_profit_calculation() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // ATR = $2.00, multiplier = 1.5, quantity = 10
        // Expected profit = $2.00 * 1.5 * 10 = $30.00
        let expected_profit = evaluator.calculate_expected_profit(&proposal, 2.0, 1.5);
        assert_eq!(expected_profit, dec!(30.0));
    }

    #[test]
    fn test_profit_cost_ratio() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.30 (with half-spread)
        // Expected profit: $6.50
        // Ratio: $6.50 / $1.30 = 5.0
        let ratio = evaluator.get_profit_cost_ratio(&proposal, dec!(6.50));
        assert!((ratio - 5.0).abs() < 0.01); // Float comparison with tolerance
    }

    #[test]
    fn test_default_constructor() {
        let evaluator = CostEvaluator::default();
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        let costs = evaluator.evaluate(&proposal);

        // Should use default parameters
        assert!(costs.total_cost > Decimal::ZERO);
        assert_eq!(costs.commission, dec!(0.05)); // 10 * 0.005
    }

    #[test]
    fn test_large_trade_costs() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(500.0), dec!(100.0)); // $50,000 trade

        let costs = evaluator.evaluate(&proposal);

        // Commission: 100 * 0.005 = $0.50
        assert_eq!(costs.commission, dec!(0.5));

        // Slippage: $50,000 * 0.001 = $50.00
        assert_eq!(costs.estimated_slippage, dec!(50.0));

        // Spread: $50,000 * (5/2 / 10000) = $12.50 (HALF-SPREAD)
        assert_eq!(costs.spread_cost, dec!(12.5));

        // Total: $0.50 + $50.00 + $12.50 = $63.00
        assert_eq!(costs.total_cost, dec!(63.0));
    }

    #[test]
    fn test_zero_quantity_edge_case() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(0.0));

        let costs = evaluator.evaluate(&proposal);

        // All costs should be zero for zero quantity
        assert_eq!(costs.total_cost, Decimal::ZERO);
        assert_eq!(costs.commission, Decimal::ZERO);
    }

    #[test]
    fn test_high_profit_ratio_requirement() {
        use crate::domain::trading::fee_model::ConstantFeeModel;
        let evaluator = CostEvaluator::new(
            Arc::new(ConstantFeeModel::new(dec!(0.005), dec!(0.001))),
            5.0
        );
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Costs: $1.55
        // Ratio: 5.0 (very conservative)
        // Min threshold: $1.55 * 5.0 = $7.75
        // Expected profit: $10.00 > $7.75 ✅
        assert!(evaluator.is_profitable(&proposal, dec!(10.0), 5.0));

        // Expected profit: $5.00 < $7.75 ❌
        assert!(!evaluator.is_profitable(&proposal, dec!(5.0), 5.0));
    }
}
