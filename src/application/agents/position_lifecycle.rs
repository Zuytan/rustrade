//! Position Lifecycle Handler
//!
//! Manages position-related operations including:
//! - Pending order management and timeout handling
//! - Trailing stop initialization and updates
//! - Position state synchronization
//!
//! Extracted from [`Analyst`] to reduce module complexity.

use crate::domain::ports::ExecutionService;
use crate::domain::trading::symbol_context::SymbolContext;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tracing::{error, info};

/// Manages pending orders and handles timeouts.
///
/// Checks if a pending order has timed out and attempts to cancel
/// orphaned orders on the exchange.
///
/// # Arguments
/// * `execution_service` - Service to interact with the exchange
/// * `context` - Symbol context with position manager state
/// * `symbol` - Trading symbol
/// * `timestamp` - Current timestamp
/// * `timeout_ms` - Timeout duration in milliseconds (default: 60000)
pub async fn manage_pending_orders(
    execution_service: &Arc<dyn ExecutionService>,
    context: &mut SymbolContext,
    symbol: &str,
    timestamp: i64,
    timeout_ms: i64,
) {
    if context
        .position_manager
        .check_timeout(timestamp, timeout_ms)
    {
        info!(
            "PositionLifecycle [{}]: Pending order TIMEOUT detected. Checking open orders to CANCEL...",
            symbol
        );

        // 1. Fetch Open Orders
        match execution_service.get_open_orders().await {
            Ok(orders) => {
                // 2. Find orders for this symbol
                let symbol_orders: Vec<_> = orders.iter().filter(|o| o.symbol == symbol).collect();

                if symbol_orders.is_empty() {
                    info!(
                        "PositionLifecycle [{}]: No open orders found on exchange. Clearing local pending state.",
                        symbol
                    );
                    context.position_manager.clear_pending();
                } else {
                    // 3. Cancel them
                    for order in symbol_orders {
                        info!(
                            "PositionLifecycle [{}]: Cancelling orphaned order {}...",
                            symbol, order.id
                        );
                        if let Err(e) = execution_service.cancel_order(&order.id, symbol).await {
                            error!(
                                "PositionLifecycle [{}]: Failed to cancel order {}: {}",
                                symbol, order.id, e
                            );
                        }
                    }
                    // Order status update will clear pending state via subscription
                }
            }
            Err(e) => {
                error!(
                    "PositionLifecycle [{}]: Failed to fetch open orders: {}",
                    symbol, e
                );
            }
        }
    }
}

/// Auto-initializes trailing stop for existing positions.
///
/// Handles cases where:
/// - Position existed from previous session
/// - Position was created manually
/// - Analyst restarted after Buy but before position was closed
///
/// # Arguments
/// * `context` - Symbol context to modify
/// * `symbol` - Trading symbol for logging
/// * `entry_price` - Position entry price
/// * `atr` - Current ATR value
pub fn initialize_trailing_stop_if_needed(
    context: &mut SymbolContext,
    symbol: &str,
    entry_price: Decimal,
    atr: Option<Decimal>,
) {
    // Only initialize if no trailing stop is active
    if context.position_manager.trailing_stop.is_active() {
        return;
    }

    let atr_val = atr.unwrap_or(dec!(1.0));
    let multiplier = context.config.trailing_stop_atr_multiplier;

    context.position_manager.trailing_stop =
        crate::application::risk_management::trailing_stops::StopState::on_buy(
            entry_price,
            atr_val,
            multiplier,
        );

    if let Some(stop_price) = context.position_manager.trailing_stop.get_stop_price() {
        info!(
            "PositionLifecycle [{}]: Auto-initialized trailing stop (entry={}, stop={}, atr={})",
            symbol, entry_price, stop_price, atr_val
        );
    }
}

/// Initializes trailing stop immediately after a BUY order is placed.
///
/// Uses current price and ATR to establish the initial stop level.
pub fn initialize_trailing_stop_on_buy(context: &mut SymbolContext, price: Decimal) {
    if let Some(atr) = context.last_features.atr
        && atr > Decimal::ZERO
    {
        let atr_decimal = atr;
        let multiplier = context.config.trailing_stop_atr_multiplier;

        context.position_manager.trailing_stop =
            crate::application::risk_management::trailing_stops::StopState::on_buy(
                price,
                atr_decimal,
                multiplier,
            );
    }
}

/// Checks trailing stop and returns exit signal if triggered.
///
/// # Arguments
/// * `context` - Symbol context with position manager
/// * `symbol` - Trading symbol
/// * `current_price` - Current market price
///
/// # Returns
/// Optional OrderSide::Sell if trailing stop triggered.
pub fn check_trailing_stop(
    context: &mut SymbolContext,
    symbol: &str,
    current_price: Decimal,
) -> Option<crate::domain::trading::types::OrderSide> {
    let atr_decimal = context.last_features.atr.unwrap_or(Decimal::ZERO);
    let multiplier_decimal = context.config.trailing_stop_atr_multiplier;

    context.position_manager.check_trailing_stop(
        symbol,
        current_price,
        atr_decimal,
        multiplier_decimal,
    )
}

/// Synchronizes local position state with portfolio data.
///
/// Updates position manager state based on actual portfolio holdings.
///
/// # Arguments
/// * `context` - Symbol context to update
/// * `symbol` - Trading symbol
/// * `has_position` - Whether portfolio has a position in this symbol
pub fn sync_position_state(context: &mut SymbolContext, symbol: &str, has_position: bool) {
    context
        .position_manager
        .ack_pending_orders(has_position, symbol);

    // Reset taken_profit flag when position is closed
    if !has_position {
        context.taken_profit = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agents::analyst_config::AnalystConfig;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::DualSMAStrategy;
    use crate::domain::trading::symbol_context::SymbolContext;
    use rust_decimal_macros::dec;
    use std::sync::Arc;

    fn create_test_context() -> SymbolContext {
        let config = AnalystConfig::default();
        let strategy = Arc::new(DualSMAStrategy::new(20, 60, dec!(0.0)));
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        SymbolContext::new(config, strategy, win_rate_provider, vec![])
    }

    #[test]
    fn test_sync_position_state_resets_taken_profit() {
        let mut context = create_test_context();
        context.taken_profit = true;

        sync_position_state(&mut context, "TEST", false);

        assert!(!context.taken_profit);
    }

    #[test]
    fn test_sync_position_state_preserves_taken_profit_with_position() {
        let mut context = create_test_context();
        context.taken_profit = true;

        sync_position_state(&mut context, "TEST", true);

        assert!(context.taken_profit);
    }

    #[test]
    fn test_initialize_trailing_stop_skips_if_active() {
        let mut context = create_test_context();

        // Manually activate a trailing stop
        context.position_manager.trailing_stop =
            crate::application::risk_management::trailing_stops::StopState::on_buy(
                Decimal::from(100),
                Decimal::from(2),
                Decimal::from(3),
            );

        let original_stop = context.position_manager.trailing_stop.get_stop_price();

        // Try to initialize again with different values
        initialize_trailing_stop_if_needed(
            &mut context,
            "TEST",
            Decimal::from(200), // Different entry
            Some(dec!(5.0)),    // Different ATR
        );

        // Stop should not have changed
        assert_eq!(
            context.position_manager.trailing_stop.get_stop_price(),
            original_stop
        );
    }
}
