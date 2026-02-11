use crate::domain::ports::ExecutionService;
use crate::domain::trading::symbol_context::SymbolContext;
use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tracing::debug;

use crate::application::risk_management::sizing_engine::SizingEngine;

/// Service responsible for generating and processing trading signals.
///
/// This service handles:
/// - Generating signals from trading strategies
/// - Building trade proposals with quantity calculation
/// - Applying signal filters (RSI, trailing stop)
pub struct SignalProcessor {
    sizing_engine: Arc<SizingEngine>,
}

impl SignalProcessor {
    pub fn new(sizing_engine: Arc<SizingEngine>) -> Self {
        Self { sizing_engine }
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
        position: Option<crate::application::strategies::PositionInfo>,
    ) -> Option<crate::application::strategies::Signal> {
        context.signal_generator.generate_signal(
            symbol,
            price,
            timestamp,
            &context.last_features,
            &context.strategy,
            context.config.sma_threshold,
            has_position,
            position,
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
    ///
    /// For SELL orders, uses the actual position quantity (not a calculated size).
    #[allow(clippy::too_many_arguments)]
    pub async fn build_proposal(
        &self,
        config: &super::analyst::AnalystConfig,
        execution_service: &Arc<dyn ExecutionService>,
        symbol: String,
        signal: crate::application::strategies::Signal,
        price: Decimal,
        timestamp: i64,
    ) -> Option<TradeProposal> {
        // For SELL orders, use position quantity (sell what we own)
        // For BUY orders, calculate new position size
        let quantity = match signal.side {
            OrderSide::Sell => {
                // Get actual position quantity from portfolio
                let portfolio = match execution_service.get_portfolio().await {
                    Ok(p) => p,
                    Err(e) => {
                        debug!(
                            "SignalProcessor [{}]: Failed to get portfolio for sell: {}",
                            symbol, e
                        );
                        return None;
                    }
                };

                match portfolio.positions.get(&symbol) {
                    Some(pos) if pos.quantity > Decimal::ZERO => pos.quantity,
                    _ => {
                        debug!(
                            "SignalProcessor [{}]: No position to sell. Skipping proposal.",
                            symbol
                        );
                        return None;
                    }
                }
            }
            OrderSide::Buy => {
                self.calculate_trade_quantity(config, execution_service, &symbol, price)
                    .await
            }
        };

        if quantity <= Decimal::ZERO {
            debug!(
                "SignalProcessor [{}]: Quantity is ZERO. Skipping proposal.",
                symbol
            );
            return None;
        }

        Some(TradeProposal {
            symbol,
            side: signal.side,
            price,
            quantity,
            order_type: OrderType::Market,
            reason: signal.reason,
            timestamp,
            stop_loss: signal.suggested_stop_loss,
            take_profit: signal.suggested_take_profit,
        })
    }

    /// Calculate trade quantity based on position sizing rules.
    ///
    /// Uses the execution service to get current portfolio state and calculates
    /// appropriate position size based on configuration.
    async fn calculate_trade_quantity(
        &self,
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
        let available_cash = portfolio.cash;

        let sizing_config = crate::application::risk_management::sizing_engine::SizingConfig {
            risk_per_trade_percent: config.risk_per_trade_percent,
            max_positions: config.max_positions,
            max_position_size_pct: config.max_position_size_pct,
            static_trade_quantity: config.trade_quantity,
            enable_vol_targeting: false,   // Disabled by default for now
            target_volatility: dec!(0.15), // 15% target if enabled
        };

        self.sizing_engine.calculate_quantity_with_slippage(
            &sizing_config,
            total_equity,
            price,
            symbol,
            None,                 // No volatility targeting in signal processor for now
            None, // Kelly stats can be wired from portfolio trade_history when available
            None, // Halt level can be wired from risk state when available
            None, // Regime can be wired from candle pipeline / regime detector when available
            Some(available_cash), // Cap by available cash
        )
    }

    /// Apply RSI filter to buy signals.
    pub fn apply_rsi_filter(
        signal: Option<crate::application::strategies::Signal>,
        context: &SymbolContext,
        symbol: &str,
    ) -> Option<crate::application::strategies::Signal> {
        match &signal {
            Some(s) if s.side == OrderSide::Buy => {
                if let Some(rsi) = context.last_features.rsi
                    && rsi > context.config.rsi_threshold
                {
                    debug!(
                        "SignalProcessor: Buy signal BLOCKED for {} - RSI {} > {} (Overbought)",
                        symbol, rsi, context.config.rsi_threshold
                    );
                    return None;
                }
            }
            _ => {}
        }
        signal
    }

    /// Suppress sell signals when trailing stop is active.
    ///
    /// When a trailing stop is managing the exit, we don't want regular
    /// sell signals to interfere.
    pub fn suppress_sell_if_trailing_stop(
        signal: Option<crate::application::strategies::Signal>,
        context: &SymbolContext,
        symbol: &str,
        trailing_stop_triggered: bool,
    ) -> Option<crate::application::strategies::Signal> {
        match &signal {
            Some(s) if s.side == OrderSide::Sell => {
                if context.position_manager.trailing_stop.is_active() && !trailing_stop_triggered {
                    debug!(
                        "SignalProcessor: Sell signal SUPPRESSED for {} - Using trailing stop exit instead",
                        symbol
                    );
                    return None;
                }
            }
            _ => {}
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

        let pnl_pct = if pos.average_price > Decimal::ZERO {
            (current_price - pos.average_price) / pos.average_price
        } else {
            Decimal::ZERO
        };

        if pnl_pct >= context.config.take_profit_pct {
            let quantity_to_sell = (pos.quantity * Decimal::new(5, 1)).round_dp(4); // 50%

            if quantity_to_sell > Decimal::ZERO {
                debug!(
                    "SignalProcessor: Triggering Partial Take-Profit (50%) for {} at {}% Gain",
                    symbol,
                    pnl_pct * dec!(100.0)
                );

                return Some(TradeProposal {
                    symbol: symbol.to_string(),
                    side: OrderSide::Sell,
                    price: current_price,
                    quantity: quantity_to_sell,
                    order_type: OrderType::Market,
                    reason: format!("Partial Take-Profit (+{}%)", pnl_pct * dec!(100.0)),
                    timestamp,
                    stop_loss: None,
                    take_profit: None,
                });
            }
        }
        None
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

    use rust_decimal_macros::dec;

    fn create_test_context() -> SymbolContext {
        let config = super::super::analyst::AnalystConfig::default();
        let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        let timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        SymbolContext::new(config, strategy, win_rate_provider, timeframes)
    }

    #[allow(dead_code)]
    fn create_test_candle(symbol: &str, price: f64) -> Candle {
        let price_dec = Decimal::from_f64_retain(price).unwrap();
        Candle {
            symbol: symbol.to_string(),
            open: price_dec,
            high: price_dec * dec!(1.01),
            low: price_dec * dec!(0.99),
            close: price_dec,
            volume: dec!(1000.0),
            timestamp: 1000,
        }
    }

    #[test]
    fn test_signal_processor_creation() {
        let spread_cache =
            Arc::new(crate::application::market_data::spread_cache::SpreadCache::new());
        let sizing_engine = Arc::new(
            crate::application::risk_management::sizing_engine::SizingEngine::new(spread_cache),
        );
        let processor = SignalProcessor::new(sizing_engine);
        // Should create successfully
        let _ = processor;
    }

    #[test]
    fn test_rsi_filter_blocks_overbought() {
        let mut context = create_test_context();

        // Set RSI to overbought level
        context.last_features.rsi = Some(dec!(75.0));
        context.config.rsi_threshold = dec!(70.0);

        let signal = Some(crate::application::strategies::Signal::buy(
            "Test".to_string(),
        ));
        let filtered = SignalProcessor::apply_rsi_filter(signal, &context, "BTC/USD");

        // Should block the buy signal
        assert_eq!(filtered, None);
    }

    #[test]
    fn test_rsi_filter_allows_normal() {
        let mut context = create_test_context();

        // Set RSI to normal level
        context.last_features.rsi = Some(dec!(50.0));
        context.config.rsi_threshold = dec!(70.0);

        let signal = Some(crate::application::strategies::Signal::buy(
            "Test".to_string(),
        ));
        let filtered = SignalProcessor::apply_rsi_filter(signal, &context, "BTC/USD");

        // Should allow the buy signal
        assert_eq!(
            filtered.map(|s| s.side),
            Some(crate::domain::trading::types::OrderSide::Buy)
        );
    }

    #[test]
    fn test_rsi_filter_ignores_sell() {
        let mut context = create_test_context();

        // Set RSI to overbought level
        context.last_features.rsi = Some(dec!(75.0));
        context.config.rsi_threshold = dec!(70.0);

        let signal = Some(crate::application::strategies::Signal::sell(
            "Test".to_string(),
        ));
        let filtered = SignalProcessor::apply_rsi_filter(signal, &context, "BTC/USD");

        // Should not affect sell signals
        assert_eq!(
            filtered.map(|s| s.side),
            Some(crate::domain::trading::types::OrderSide::Sell)
        );
    }

    #[test]
    fn test_trailing_stop_suppression() {
        let mut context = create_test_context();

        // Activate trailing stop
        context.position_manager.trailing_stop =
            crate::application::risk_management::trailing_stops::StopState::on_buy(
                dec!(50000),
                dec!(100),
                dec!(2),
            );

        let signal = Some(crate::application::strategies::Signal::sell(
            "Test".to_string(),
        ));
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
                dec!(50000),
                dec!(100),
                dec!(2),
            );

        let signal = Some(crate::application::strategies::Signal::sell(
            "Test".to_string(),
        ));
        let filtered = SignalProcessor::suppress_sell_if_trailing_stop(
            signal, &context, "BTC/USD", true, // trailing stop triggered
        );

        // Should allow the sell signal when trailing stop triggered
        assert_eq!(
            filtered.map(|s| s.side),
            Some(crate::domain::trading::types::OrderSide::Sell)
        );
    }
}
