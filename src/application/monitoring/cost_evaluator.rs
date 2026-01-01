use crate::domain::trading::types::TradeProposal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;

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
    /// Commission per share (e.g., $0.005 per share)
    commission_per_share: Decimal,
    /// Slippage as percentage of trade value (e.g., 0.001 = 0.1%)
    slippage_pct: Decimal,
    /// Spread in basis points (e.g., 5.0 = 0.05%)
    spread_bps: Decimal,
}

impl CostEvaluator {
    /// Create a new CostEvaluator with specified cost parameters
    ///
    /// # Arguments
    /// * `commission_per_share` - Commission fee per share (e.g., 0.005 for $0.005/share)
    /// * `slippage_pct` - Expected slippage as decimal (e.g., 0.001 for 0.1%)
    /// * `spread_bps` - Bid-ask spread in basis points (e.g., 5.0 for 5 bps)
    pub fn new(commission_per_share: f64, slippage_pct: f64, spread_bps: f64) -> Self {
        Self {
            commission_per_share: Decimal::from_f64(commission_per_share).unwrap_or(Decimal::ZERO),
            slippage_pct: Decimal::from_f64(slippage_pct).unwrap_or(Decimal::ZERO),
            spread_bps: Decimal::from_f64(spread_bps).unwrap_or(Decimal::ZERO),
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
        // Commission: quantity * per-share fee
        let commission = proposal.quantity * self.commission_per_share;

        // Trade value (notional value of the trade)
        let trade_value = proposal.price * proposal.quantity;

        // Slippage: trade_value * slippage_pct
        // This represents the expected price impact when executing the order
        let estimated_slippage = trade_value * self.slippage_pct;

        // Spread: trade_value * (spread_bps / 10000)
        // Convert basis points to decimal (1 bp = 0.01% = 0.0001)
        let spread_cost = trade_value * (self.spread_bps / Decimal::from(10000));

        // Total cost is sum of all components
        let total_cost = commission + estimated_slippage + spread_cost;

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
        Self::new(0.005, 0.001, 5.0)
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
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        let costs = evaluator.evaluate(&proposal);

        // Commission: 10 shares * $0.005 = $0.05
        assert_eq!(costs.commission, dec!(0.05));

        // Slippage: $1000 (trade value) * 0.001 = $1.00
        assert_eq!(costs.estimated_slippage, dec!(1.0));

        // Spread: $1000 * (5 / 10000) = $0.50
        assert_eq!(costs.spread_cost, dec!(0.5));

        // Total: $0.05 + $1.00 + $0.50 = $1.55
        assert_eq!(costs.total_cost, dec!(1.55));
    }

    #[test]
    fn test_profitability_check_pass() {
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.55 (from previous test)
        // Min threshold: $1.55 * 2.0 = $3.10
        // Expected profit: $5.00 > $3.10 ✅
        assert!(evaluator.is_profitable(&proposal, dec!(5.0), 2.0));
    }

    #[test]
    fn test_profitability_check_fail() {
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.55
        // Min threshold: $1.55 * 2.0 = $3.10
        // Expected profit: $2.00 < $3.10 ❌
        assert!(!evaluator.is_profitable(&proposal, dec!(2.0), 2.0));
    }

    #[test]
    fn test_profitability_exact_threshold() {
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.55
        // Min threshold: $1.55 * 2.0 = $3.10
        // Expected profit: $3.10 = $3.10 ✅ (equal passes)
        assert!(evaluator.is_profitable(&proposal, dec!(3.10), 2.0));
    }

    #[test]
    fn test_expected_profit_calculation() {
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // ATR = $2.00, multiplier = 1.5, quantity = 10
        // Expected profit = $2.00 * 1.5 * 10 = $30.00
        let expected_profit = evaluator.calculate_expected_profit(&proposal, 2.0, 1.5);
        assert_eq!(expected_profit, dec!(30.0));
    }

    #[test]
    fn test_profit_cost_ratio() {
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(100.0), dec!(10.0));

        // Total costs: $1.55
        // Expected profit: $7.75
        // Ratio: $7.75 / $1.55 = 5.0
        let ratio = evaluator.get_profit_cost_ratio(&proposal, dec!(7.75));
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
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(500.0), dec!(100.0)); // $50,000 trade

        let costs = evaluator.evaluate(&proposal);

        // Commission: 100 * 0.005 = $0.50
        assert_eq!(costs.commission, dec!(0.5));

        // Slippage: $50,000 * 0.001 = $50.00
        assert_eq!(costs.estimated_slippage, dec!(50.0));

        // Spread: $50,000 * 0.0005 = $25.00
        assert_eq!(costs.spread_cost, dec!(25.0));

        // Total: $75.50
        assert_eq!(costs.total_cost, dec!(75.5));
    }

    #[test]
    fn test_zero_quantity_edge_case() {
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
        let proposal = create_test_proposal(dec!(100.0), dec!(0.0));

        let costs = evaluator.evaluate(&proposal);

        // All costs should be zero for zero quantity
        assert_eq!(costs.total_cost, Decimal::ZERO);
        assert_eq!(costs.commission, Decimal::ZERO);
    }

    #[test]
    fn test_high_profit_ratio_requirement() {
        let evaluator = CostEvaluator::new(0.005, 0.001, 5.0);
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
