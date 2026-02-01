use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::info;

use crate::application::agents::signal_processor::SignalProcessor;
use crate::application::trading::trade_filter::TradeFilter;
use crate::domain::market::market_regime::MarketRegime;
use crate::domain::ports::ExecutionService;
use crate::domain::trading::symbol_context::SymbolContext;
use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};

/// Service responsible for evaluating trade signals and generating proposals.
///
/// Encapsulates the logic for:
/// - Post-signal validation (Long-Only, Pending, Cooldown)
/// - Expectancy evaluation
/// - Minimum hold time checks
/// - Trade proposal construction (quantity, order type)
/// - Cost-aware profitability analysis
pub struct TradeEvaluator {
    trade_filter: TradeFilter,
    signal_processor: SignalProcessor,
}
pub struct EvaluationInput<'a> {
    pub signal: OrderSide,
    pub symbol: &'a str,
    pub price: Decimal,
    pub timestamp: i64,
    pub regime: &'a MarketRegime,
    pub execution_service: &'a Arc<dyn ExecutionService>,
    pub has_position: bool,
}

impl TradeEvaluator {
    pub fn new(trade_filter: TradeFilter, signal_processor: SignalProcessor) -> Self {
        Self {
            trade_filter,
            signal_processor,
        }
    }

    /// Evaluate a signal and generate a valid trade proposal if it passes all checks.
    pub async fn evaluate_and_propose(
        &self,
        context: &mut SymbolContext,
        input: EvaluationInput<'_>,
    ) -> Option<TradeProposal> {
        // 1. Basic Signal Validation (Long-Only, Pending, Cooldown)
        // 1. Basic Signal Validation (Long-Only, Pending, Cooldown)
        if !self.trade_filter.validate_signal(
            input.signal,
            input.symbol,
            &context.position_manager,
            &context.config,
            input.timestamp,
            input.has_position,
        ) {
            return None;
        }

        // 2. Execution Logic (Expectancy & Quantity)
        context.position_manager.last_signal_time = input.timestamp;

        // Use already calculated regime for expectancy
        let expectancy = context
            .expectancy_evaluator
            .evaluate(input.symbol, input.price, input.regime)
            .await;

        let risk_ratio = if expectancy.reward_risk_ratio > Decimal::ZERO {
            expectancy.reward_risk_ratio
        } else {
            context.cached_reward_risk_ratio
        };

        // Validate using calculated or cached ratio
        if !self
            .trade_filter
            .validate_expectancy(input.symbol, risk_ratio)
        {
            return None;
        }

        // Check minimum hold time for sell signals
        if !self.trade_filter.validate_min_hold_time(
            input.signal,
            input.symbol,
            input.timestamp,
            context.last_entry_time,
            context.min_hold_time_ms,
        ) {
            return None;
        }

        let order_type = match input.signal {
            OrderSide::Buy => OrderType::Limit,
            OrderSide::Sell => OrderType::Market,
        };

        let reason = format!(
            "Strategy: {} (Regime: {})",
            context.active_strategy_mode, input.regime.regime_type
        );

        // 3. Build Proposal via SignalProcessor
        let mut proposal = match self
            .signal_processor
            .build_proposal(
                &context.config,
                input.execution_service,
                input.symbol.to_string(),
                input.signal,
                input.price,
                input.timestamp,
                reason,
            )
            .await
        {
            Some(p) => p,
            None => return None,
        };

        proposal.order_type = order_type;

        // 4. Cost-Aware Trading Filter
        let atr = context.last_features.atr.unwrap_or(Decimal::ZERO);

        info!(
            "Analyst [{}]: Calculating Profit Expectancy - ATR={}, Multiplier={}, Quantity={}",
            input.symbol, atr, context.config.profit_target_multiplier, proposal.quantity
        );

        // Use fresh expectancy value if available
        let expected_profit = if expectancy.expected_value > Decimal::ZERO {
            expectancy.expected_value * proposal.quantity
        } else {
            self.trade_filter.calculate_expected_profit(
                &proposal,
                atr,
                context.config.profit_target_multiplier,
            )
        };

        let costs = self.trade_filter.evaluate_costs(&proposal);

        if !self.trade_filter.validate_profitability(
            &proposal,
            expected_profit,
            costs.total_cost,
            context.config.min_profit_ratio,
            input.symbol,
        ) {
            return None;
        }

        Some(proposal)
    }
}
