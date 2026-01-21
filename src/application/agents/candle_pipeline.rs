//! Candle Processing Pipeline
//!
//! Refactored from [`Analyst::process_candle`] to provide discrete, testable stages
//! for processing market candles and generating trade proposals.
//!
//! ## Architecture
//!
//! The pipeline breaks down candle processing into 6 distinct stages:
//! 1. **Regime Analysis** - Detect market regime and apply dynamic risk scaling
//! 2. **Indicator Updates** - Update technical indicators and features
//! 3. **Position Synchronization** - Sync local state with portfolio
//! 4. **Trailing Stop Management** - Check and manage trailing stops
//! 5. **Signal Generation** - Generate and filter trading signals
//! 6. **Trade Evaluation** - Validate and create trade proposals

use crate::application::agents::trade_evaluator::{EvaluationInput, TradeEvaluator};
use crate::domain::market::market_regime::MarketRegime;
use crate::domain::ports::ExecutionService;
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::symbol_context::SymbolContext;
use crate::domain::trading::types::{Candle, OrderSide, TradeProposal};
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::debug;

/// Pipeline context containing all data needed for candle processing
pub struct PipelineContext<'a> {
    pub symbol: &'a str,
    pub candle: &'a Candle,
    pub context: &'a mut SymbolContext,
    pub portfolio: Option<&'a Portfolio>,
}

/// Candle processing pipeline
pub struct CandlePipeline {
    execution_service: Arc<dyn ExecutionService>,
    candle_repository: Option<Arc<dyn CandleRepository>>,
    trade_evaluator: TradeEvaluator,
}

impl CandlePipeline {
    /// Create a new candle processing pipeline
    pub fn new(
        execution_service: Arc<dyn ExecutionService>,
        candle_repository: Option<Arc<dyn CandleRepository>>,
        trade_evaluator: TradeEvaluator,
    ) -> Self {
        Self {
            execution_service,
            candle_repository,
            trade_evaluator,
        }
    }

    /// Process a candle through the complete pipeline
    ///
    /// Returns a trade proposal if all stages pass validation
    pub async fn process(&self, ctx: &mut PipelineContext<'_>) -> Option<TradeProposal> {
        // Stage 1: Regime Analysis
        let regime = self.detect_and_apply_regime(ctx).await;

        // Stage 2: Indicator Updates
        self.update_indicators(ctx);

        // Stage 3: Position Synchronization
        let has_position = self.sync_position_state(ctx);

        // Stage 4: Trailing Stop Management
        if let Some(stop_signal) = self.manage_trailing_stops(ctx, has_position) {
            // Trailing stop triggered - evaluate immediately
            return self
                .evaluate_and_propose(ctx, stop_signal, &regime, has_position)
                .await;
        }

        // Stage 5: Signal Generation
        let signal = self.generate_and_filter_signal(ctx, has_position)?;

        // Stage 6: Trade Evaluation
        self.evaluate_and_propose(ctx, signal, &regime, has_position)
            .await
    }

    /// Stage 1: Detect market regime and apply dynamic risk scaling
    async fn detect_and_apply_regime(&self, ctx: &mut PipelineContext<'_>) -> MarketRegime {
        let regime = super::regime_handler::detect_market_regime(
            &self.candle_repository,
            ctx.symbol,
            ctx.candle.timestamp,
            ctx.context,
        )
        .await;

        // Apply dynamic risk scaling based on regime
        super::regime_handler::apply_dynamic_risk_scaling(ctx.context, &regime, ctx.symbol);

        // Apply adaptive strategy switching if enabled
        super::regime_handler::apply_adaptive_strategy_switching(
            ctx.context,
            &regime,
            &ctx.context.config.clone(),
            ctx.symbol,
        );

        regime
    }

    /// Stage 2: Update technical indicators
    fn update_indicators(&self, ctx: &mut PipelineContext<'_>) {
        ctx.context.update(ctx.candle);
    }

    /// Stage 3: Synchronize position state with portfolio
    ///
    /// Returns whether the symbol has an active position
    fn sync_position_state(&self, ctx: &mut PipelineContext<'_>) -> bool {
        let has_position = ctx
            .portfolio
            .map(|p| {
                p.positions
                    .get(ctx.symbol)
                    .map(|pos| pos.quantity > Decimal::ZERO)
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        // Acknowledge pending orders
        ctx.context
            .position_manager
            .ack_pending_orders(has_position, ctx.symbol);

        // Reset taken_profit flag when position is closed
        if !has_position {
            ctx.context.taken_profit = false;
        }

        // Auto-initialize trailing stop for existing positions
        if has_position
            && !ctx.context.position_manager.trailing_stop.is_active()
            && let Some(portfolio) = ctx.portfolio
            && let Some(pos) = portfolio.positions.get(ctx.symbol)
        {
            super::position_lifecycle::initialize_trailing_stop_if_needed(
                ctx.context,
                ctx.symbol,
                pos.average_price,
                ctx.context.last_features.atr,
            );
        }

        has_position
    }

    /// Stage 4: Manage trailing stops and check for exit signals
    ///
    /// Returns Some(OrderSide::Sell) if trailing stop is triggered
    fn manage_trailing_stops(
        &self,
        ctx: &mut PipelineContext<'_>,
        has_position: bool,
    ) -> Option<OrderSide> {
        if !has_position {
            return None;
        }

        let atr_decimal = Decimal::from_f64_retain(ctx.context.last_features.atr.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO);
        let multiplier_decimal =
            Decimal::from_f64_retain(ctx.context.config.trailing_stop_atr_multiplier)
                .unwrap_or(Decimal::from(3));

        let signal = ctx.context.position_manager.check_trailing_stop(
            ctx.symbol,
            ctx.candle.close,
            atr_decimal,
            multiplier_decimal,
        );

        // Check partial take-profit if trailing stop not triggered
        #[allow(clippy::collapsible_if)]
        if signal.is_none() && has_position {
            if let Some(_proposal) =
                super::signal_processor::SignalProcessor::check_partial_take_profit(
                    ctx.context,
                    ctx.symbol,
                    ctx.candle.close,
                    ctx.candle.timestamp * 1000,
                    ctx.portfolio.map(|p| &p.positions),
                )
            {
                debug!(
                    "CandlePipeline [{}]: Partial take-profit triggered",
                    ctx.symbol
                );
                // Note: In the full implementation, this would be sent directly
                // For now, we'll handle it in the main analyst loop
            }
        }

        signal
    }

    /// Stage 5: Generate and filter trading signal
    fn generate_and_filter_signal(
        &self,
        ctx: &mut PipelineContext<'_>,
        has_position: bool,
    ) -> Option<OrderSide> {
        // Generate signal from strategy
        let mut signal = super::signal_processor::SignalProcessor::generate_signal(
            ctx.context,
            ctx.symbol,
            ctx.candle.close,
            ctx.candle.timestamp * 1000,
            has_position,
        );

        // Apply RSI filter
        signal = super::signal_processor::SignalProcessor::apply_rsi_filter(
            signal,
            ctx.context,
            ctx.symbol,
        );

        // Suppress sell signals when trailing stop is active
        signal = super::signal_processor::SignalProcessor::suppress_sell_if_trailing_stop(
            signal,
            ctx.context,
            ctx.symbol,
            false, // trailing_stop_triggered is handled separately
        );

        signal
    }

    /// Stage 6: Evaluate signal and create trade proposal
    async fn evaluate_and_propose(
        &self,
        ctx: &mut PipelineContext<'_>,
        signal: OrderSide,
        regime: &MarketRegime,
        has_position: bool,
    ) -> Option<TradeProposal> {
        let input = EvaluationInput {
            signal,
            symbol: ctx.symbol,
            price: ctx.candle.close,
            timestamp: ctx.candle.timestamp * 1000,
            regime,
            execution_service: &self.execution_service,
            has_position,
        };

        let proposal = self
            .trade_evaluator
            .evaluate_and_propose(ctx.context, input)
            .await?;

        // Update position manager state
        ctx.context
            .position_manager
            .set_pending_order(signal, ctx.candle.timestamp * 1000);

        // Track entry time for buy signals
        if signal == OrderSide::Buy {
            ctx.context.last_entry_time = Some(ctx.candle.timestamp * 1000);
            super::position_lifecycle::initialize_trailing_stop_on_buy(
                ctx.context,
                ctx.candle.close,
            );
        }

        Some(proposal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agents::analyst_config::AnalystConfig;
    use crate::application::monitoring::cost_evaluator::CostEvaluator;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::DualSMAStrategy;
    use crate::application::trading::trade_filter::TradeFilter;
    use crate::domain::trading::fee_model::ConstantFeeModel;
    use crate::infrastructure::mock::MockExecutionService;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    fn create_test_pipeline() -> CandlePipeline {
        use crate::domain::trading::portfolio::Portfolio;
        use tokio::sync::RwLock;

        let portfolio = Arc::new(RwLock::new(Portfolio::new()));
        let execution_service = Arc::new(MockExecutionService::new(portfolio));
        let config = AnalystConfig::default();
        let fee_model = Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO));
        let cost_evaluator = CostEvaluator::new(fee_model, config.spread_bps);
        let trade_filter = TradeFilter::new(cost_evaluator);
        let trade_evaluator = TradeEvaluator::new(trade_filter);

        CandlePipeline::new(execution_service, None, trade_evaluator)
    }

    fn create_test_context() -> SymbolContext {
        let config = AnalystConfig::default();
        let strategy = Arc::new(DualSMAStrategy::new(20, 60, 0.0));
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        SymbolContext::new(config, strategy, win_rate_provider, vec![])
    }

    fn create_test_candle(symbol: &str, price: f64, timestamp: i64) -> Candle {
        Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(price).unwrap(),
            high: Decimal::from_f64_retain(price * 1.01).unwrap(),
            low: Decimal::from_f64_retain(price * 0.99).unwrap(),
            close: Decimal::from_f64_retain(price).unwrap(),
            volume: 1000.0,
            timestamp,
        }
    }

    #[test]
    fn test_pipeline_creation() {
        let pipeline = create_test_pipeline();
        // Should create successfully
        let _ = pipeline;
    }

    #[test]
    fn test_update_indicators() {
        let pipeline = create_test_pipeline();
        let mut context = create_test_context();
        let candle = create_test_candle("BTC/USD", 50000.0, 1000);

        let mut ctx = PipelineContext {
            symbol: "BTC/USD",
            candle: &candle,
            context: &mut context,
            portfolio: None,
        };

        pipeline.update_indicators(&mut ctx);

        // Indicators should be updated
        assert!(ctx.context.last_features.rsi.is_some());
    }

    #[test]
    fn test_sync_position_state_no_position() {
        let pipeline = create_test_pipeline();
        let mut context = create_test_context();
        let candle = create_test_candle("BTC/USD", 50000.0, 1000);

        context.taken_profit = true; // Set to true to test reset

        let mut ctx = PipelineContext {
            symbol: "BTC/USD",
            candle: &candle,
            context: &mut context,
            portfolio: None,
        };

        let has_position = pipeline.sync_position_state(&mut ctx);

        assert!(!has_position);
        assert!(!ctx.context.taken_profit); // Should be reset
    }

    #[test]
    fn test_manage_trailing_stops_no_position() {
        let pipeline = create_test_pipeline();
        let mut context = create_test_context();
        let candle = create_test_candle("BTC/USD", 50000.0, 1000);

        let mut ctx = PipelineContext {
            symbol: "BTC/USD",
            candle: &candle,
            context: &mut context,
            portfolio: None,
        };

        let signal = pipeline.manage_trailing_stops(&mut ctx, false);

        assert!(signal.is_none());
    }

    #[test]
    fn test_generate_and_filter_signal() {
        let pipeline = create_test_pipeline();
        let mut context = create_test_context();
        let candle = create_test_candle("BTC/USD", 50000.0, 1000);

        // Warm up indicators
        for i in 0..100 {
            let c = create_test_candle("BTC/USD", 50000.0 + i as f64, i);
            context.update(&c);
        }

        let mut ctx = PipelineContext {
            symbol: "BTC/USD",
            candle: &candle,
            context: &mut context,
            portfolio: None,
        };

        let signal = pipeline.generate_and_filter_signal(&mut ctx, false);

        // Signal may or may not be generated depending on strategy
        // Just verify it doesn't panic
        let _ = signal;
    }
}
