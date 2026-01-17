use crate::domain::ports::ExecutionService;
use crate::domain::trading::symbol_context::SymbolContext;
use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use tracing::debug;

/// Service responsible for generating and processing trading signals.
///
/// This service handles:
/// - Generating signals from trading strategies
/// - Building trade proposals with quantity calculation
/// - Applying signal filters (RSI, trailing stop)
pub struct SignalProcessor;

impl SignalProcessor {
    pub fn new() -> Self {
        Self
    }

    /// Generate trading signal from strategy.
    ///
    /// Delegates to the context's signal generator which applies the trading strategy
    /// to current market conditions and features.
    pub fn generate_signal(
        context: &mut SymbolContext,
        symbol: &str,
        price: Decimal,
        timestamp: i64,
        has_position: bool,
    ) -> Option<OrderSide> {
        context.signal_generator.generate_signal(
            symbol,
            price,
            timestamp,
            &context.last_features,
            &context.strategy,
            context.config.sma_threshold,
            has_position,
            context.last_macd_histogram,
            &context.candle_history,
            &context.rsi_history,
            // Pass OFI values from context
            context.ofi_value,
            context.cumulative_delta.value,
            context.volume_profile.clone(),
            &context.ofi_history,
        )
    }

    /// Build trade proposal from signal.
    ///
    /// Calculates appropriate position size and creates a complete trade proposal
    /// ready to be sent to the risk manager.
    pub async fn build_proposal(
        config: &super::analyst::AnalystConfig,
        execution_service: &Arc<dyn ExecutionService>,
        symbol: String,
        side: OrderSide,
        price: Decimal,
        timestamp: i64,
        reason: String,
    ) -> Option<TradeProposal> {
        // Calculate quantity
        let quantity =
            Self::calculate_trade_quantity(config, execution_service, &symbol, price).await;

        if quantity <= Decimal::ZERO {
            debug!(
                "SignalProcessor [{}]: Quantity is ZERO. Skipping proposal.",
                symbol
            );
            return None;
        }

        Some(TradeProposal {
            symbol,
            side,
            price,
            quantity,
            order_type: OrderType::Market,
            reason,
            timestamp,
        })
    }

    /// Calculate trade quantity based on position sizing rules.
    ///
    /// Uses the execution service to get current portfolio state and calculates
    /// appropriate position size based on configuration.
    async fn calculate_trade_quantity(
        config: &super::analyst::AnalystConfig,
        execution_service: &Arc<dyn ExecutionService>,
        symbol: &str,
        price: Decimal,
    ) -> Decimal {
        let portfolio = match execution_service.get_portfolio().await {
            Ok(p) => p,
            Err(e) => {
                debug!("SignalProcessor: Failed to fetch portfolio: {}", e);
                return Decimal::ZERO;
            }
        };

        // Get current prices for total equity calculation
        let mut current_prices = std::collections::HashMap::new();
        current_prices.insert(symbol.to_string(), price);

        let total_equity = portfolio.total_equity(&current_prices);

        let sizing_config = crate::application::risk_management::sizing_engine::SizingConfig {
            risk_per_trade_percent: config.risk_per_trade_percent,
            max_positions: config.max_positions,
            max_position_size_pct: config.max_position_size_pct,
            static_trade_quantity: config.trade_quantity,
        };

        crate::application::risk_management::sizing_engine::SizingEngine::calculate_quantity(
            &sizing_config,
            total_equity,
            price,
            symbol,
        )
    }

    /// Apply RSI filter to buy signals.
    ///
    /// Blocks buy signals when RSI is above the overbought threshold.
    pub fn apply_rsi_filter(
        signal: Option<OrderSide>,
        context: &SymbolContext,
        symbol: &str,
    ) -> Option<OrderSide> {
        if let Some(OrderSide::Buy) = signal
            && let Some(rsi) = context.last_features.rsi
            && rsi > context.config.rsi_threshold
        {
            debug!(
                "SignalProcessor: Buy signal BLOCKED for {} - RSI {:.2} > {:.2} (Overbought)",
                symbol, rsi, context.config.rsi_threshold
            );
            return None;
        }
        signal
    }

    /// Suppress sell signals when trailing stop is active.
    ///
    /// When a trailing stop is managing the exit, we don't want regular
    /// sell signals to interfere.
    pub fn suppress_sell_if_trailing_stop(
        signal: Option<OrderSide>,
        context: &SymbolContext,
        symbol: &str,
        trailing_stop_triggered: bool,
    ) -> Option<OrderSide> {
        if let Some(OrderSide::Sell) = signal
            && context.position_manager.trailing_stop.is_active()
            && !trailing_stop_triggered
        {
            debug!(
                "SignalProcessor: Sell signal SUPPRESSED for {} - Using trailing stop exit instead",
                symbol
            );
            return None;
        }
        signal
    }

    /// Check if partial take-profit conditions are met.
    ///
    /// Returns a TradeProposal for a partial sell if:
    /// - Position exists and has quantity
    /// - PnL exceeds take_profit_pct
    /// - Partial profit hasn't been taken yet
    pub fn check_partial_take_profit(
        context: &SymbolContext,
        symbol: &str,
        current_price: Decimal,
        timestamp: i64,
        portfolio_positions: Option<
            &std::collections::HashMap<String, crate::domain::trading::portfolio::Position>,
        >,
    ) -> Option<TradeProposal> {
        if context.taken_profit {
            return None;
        }

        let positions = portfolio_positions?;
        let pos = positions.get(symbol)?;

        if pos.quantity <= Decimal::ZERO {
            return None;
        }

        let price_f64 = current_price.to_f64().unwrap_or(0.0);
        let avg_price = pos.average_price.to_f64().unwrap_or(1.0);

        let pnl_pct = if avg_price != 0.0 {
            (price_f64 - avg_price) / avg_price
        } else {
            0.0
        };

        if pnl_pct >= context.config.take_profit_pct {
            let quantity_to_sell = (pos.quantity * Decimal::new(5, 1)).round_dp(4); // 50%

            if quantity_to_sell > Decimal::ZERO {
                debug!(
                    "SignalProcessor: Triggering Partial Take-Profit (50%) for {} at {:.2}% Gain",
                    symbol,
                    pnl_pct * 100.0
                );

                return Some(TradeProposal {
                    symbol: symbol.to_string(),
                    side: OrderSide::Sell,
                    price: current_price,
                    quantity: quantity_to_sell,
                    order_type: OrderType::Market,
                    reason: format!("Partial Take-Profit (+{:.2}%)", pnl_pct * 100.0),
                    timestamp,
                });
            }
        }
        None
    }
}

impl Default for SignalProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::StrategyFactory;
    use crate::domain::market::strategy_config::StrategyMode;
    use crate::domain::trading::types::Candle;
    use rust_decimal::Decimal;

    fn create_test_context() -> SymbolContext {
        let config = super::super::analyst::AnalystConfig::default();
        let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        let timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        SymbolContext::new(config, strategy, win_rate_provider, timeframes)
    }

    #[allow(dead_code)]
    fn create_test_candle(symbol: &str, price: f64) -> Candle {
        Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(price).unwrap(),
            high: Decimal::from_f64_retain(price * 1.01).unwrap(),
            low: Decimal::from_f64_retain(price * 0.99).unwrap(),
            close: Decimal::from_f64_retain(price).unwrap(),
            volume: 1000.0,
            timestamp: 1000,
        }
    }

    #[test]
    fn test_signal_processor_creation() {
        let processor = SignalProcessor::new();
        // Should create successfully
        let _ = processor;
    }

    #[test]
    fn test_rsi_filter_blocks_overbought() {
        let mut context = create_test_context();

        // Set RSI to overbought level
        context.last_features.rsi = Some(75.0);
        context.config.rsi_threshold = 70.0;

        let signal = Some(OrderSide::Buy);
        let filtered = SignalProcessor::apply_rsi_filter(signal, &context, "BTC/USD");

        // Should block the buy signal
        assert_eq!(filtered, None);
    }

    #[test]
    fn test_rsi_filter_allows_normal() {
        let mut context = create_test_context();

        // Set RSI to normal level
        context.last_features.rsi = Some(50.0);
        context.config.rsi_threshold = 70.0;

        let signal = Some(OrderSide::Buy);
        let filtered = SignalProcessor::apply_rsi_filter(signal, &context, "BTC/USD");

        // Should allow the buy signal
        assert_eq!(filtered, Some(OrderSide::Buy));
    }

    #[test]
    fn test_rsi_filter_ignores_sell() {
        let mut context = create_test_context();

        // Set RSI to overbought level
        context.last_features.rsi = Some(75.0);
        context.config.rsi_threshold = 70.0;

        let signal = Some(OrderSide::Sell);
        let filtered = SignalProcessor::apply_rsi_filter(signal, &context, "BTC/USD");

        // Should not affect sell signals
        assert_eq!(filtered, Some(OrderSide::Sell));
    }

    #[test]
    fn test_trailing_stop_suppression() {
        let mut context = create_test_context();

        // Activate trailing stop
        context.position_manager.trailing_stop =
            crate::application::risk_management::trailing_stops::StopState::on_buy(
                rust_decimal::Decimal::from(50000),
                rust_decimal::Decimal::from(100),
                rust_decimal::Decimal::from(2),
            );

        let signal = Some(OrderSide::Sell);
        let filtered = SignalProcessor::suppress_sell_if_trailing_stop(
            signal, &context, "BTC/USD", false, // trailing stop not triggered
        );

        // Should suppress the sell signal
        assert_eq!(filtered, None);
    }

    #[test]
    fn test_trailing_stop_allows_when_triggered() {
        let mut context = create_test_context();

        // Activate trailing stop
        context.position_manager.trailing_stop =
            crate::application::risk_management::trailing_stops::StopState::on_buy(
                rust_decimal::Decimal::from(50000),
                rust_decimal::Decimal::from(100),
                rust_decimal::Decimal::from(2),
            );

        let signal = Some(OrderSide::Sell);
        let filtered = SignalProcessor::suppress_sell_if_trailing_stop(
            signal, &context, "BTC/USD", true, // trailing stop triggered
        );

        // Should allow the sell signal when trailing stop triggered
        assert_eq!(filtered, Some(OrderSide::Sell));
    }
}
