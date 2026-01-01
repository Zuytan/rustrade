use rust_decimal::Decimal;
use tracing::{info, warn};

use crate::application::monitoring::cost_evaluator::CostEvaluator;
use crate::application::risk_management::position_manager::PositionManager;
use crate::domain::trading::types::{OrderSide, TradeProposal};

use crate::application::agents::analyst::AnalystConfig;

pub struct TradeFilter {
    cost_evaluator: CostEvaluator,
}

impl TradeFilter {
    pub fn new(cost_evaluator: CostEvaluator) -> Self {
        Self { cost_evaluator }
    }

    pub fn validate_signal(
        &self,
        signal: OrderSide,
        symbol: &str,
        position_manager: &PositionManager,
        config: &AnalystConfig,
        timestamp: i64,
        has_position: bool,
    ) -> bool {
        // 1. Long-Only Check
        if signal == OrderSide::Sell && !has_position {
            info!(
                "TradeFilter: BLOCKING Sell for {} - No position (Long-Only)",
                symbol
            );
            return false;
        }

        // 2. Pending Order Check
        if let Some(pending) = position_manager.pending_order {
            if pending == signal {
                info!(
                    "TradeFilter: Signal {:?} for {} BLOCKED - Pending Order exists",
                    signal, symbol
                );
                return false;
            }
        }

        // 3. Cooldown Check
        let cooldown_ms = config.order_cooldown_seconds * 1000;
        if timestamp - position_manager.last_signal_time < cooldown_ms as i64 {
            // validating silent reject for cooldown
            return false;
        }

        true
    }

    pub fn validate_min_hold_time(
        &self,
        signal: OrderSide,
        symbol: &str,
        timestamp: i64,
        last_entry_time: Option<i64>,
        min_hold_time_ms: i64,
    ) -> bool {
        if signal == OrderSide::Sell {
            if let Some(entry_time) = last_entry_time {
                let hold_duration_ms = timestamp - entry_time;
                if hold_duration_ms < min_hold_time_ms {
                    let remaining_minutes = (min_hold_time_ms - hold_duration_ms) / 60000;
                    info!(
                        "TradeFilter: Sell signal BLOCKED for {} - Min hold time not met ({} min remaining)",
                        symbol, remaining_minutes
                    );
                    return false;
                }
            }
        }
        true
    }

    pub fn validate_expectancy(&self, symbol: &str, reward_risk_ratio: f64) -> bool {
        if reward_risk_ratio < 0.5 {
            info!(
                "TradeFilter: Signal IGNORED for {} - Low Reward/Risk Ratio: {:.2}",
                symbol, reward_risk_ratio
            );
            return false;
        }
        true
    }

    pub fn validate_profitability(
        &self,
        proposal: &TradeProposal,
        expected_profit: Decimal,
        estimated_cost: Decimal,
        min_profit_ratio: f64,
        symbol: &str,
    ) -> bool {
        // Basic static cost check (Total Profit > Total Cost)
        if expected_profit < estimated_cost {
            info!(
                "TradeFilter: Signal IGNORED for {} - Negative Expectancy after costs",
                symbol
            );
            return false;
        }

        // Advanced CostEvaluator check
        if !self
            .cost_evaluator
            .is_profitable(proposal, expected_profit, min_profit_ratio)
        {
            let ratio = self
                .cost_evaluator
                .get_profit_cost_ratio(proposal, expected_profit);
            let costs = self.cost_evaluator.evaluate(proposal);
            warn!(
                "TradeFilter [{}]: Trade REJECTED by cost filter - Profit/Cost ratio {:.2} < {:.2} threshold (Expected Profit: ${:.2}, Total Costs: ${:.2})",
                symbol,
                ratio,
                min_profit_ratio,
                expected_profit,
                costs.total_cost
            );
            return false;
        }

        let ratio = self
            .cost_evaluator
            .get_profit_cost_ratio(proposal, expected_profit);
        let costs = self.cost_evaluator.evaluate(proposal);
        info!(
            "TradeFilter [{}]: Cost Filter PASSED - Profit/Cost ratio {:.2}x (Expected: ${:.2}, Costs: ${:.2}, Net: ${:.2})",
            symbol,
            ratio,
            expected_profit,
            costs.total_cost,
            expected_profit - costs.total_cost
        );

        true
    }

    // Helper to calculate expected profit based on ATR if available, to be passed to validate_profitability
    pub fn calculate_expected_profit(
        &self,
        proposal: &TradeProposal,
        atr: f64,
        multiplier: f64,
    ) -> Decimal {
        self.cost_evaluator
            .calculate_expected_profit(proposal, atr, multiplier)
    }
}
