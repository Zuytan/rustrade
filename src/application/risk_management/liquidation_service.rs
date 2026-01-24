//! Liquidation Service
//!
//! Handles emergency portfolio liquidation during circuit breaker events.
//! Extracted from RiskManager to follow Single Responsibility Principle.

use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::order_retry_strategy::{OrderRetryStrategy, RetryConfig};
use crate::domain::ports::MarketDataService;
use crate::domain::trading::types::{Order, OrderSide};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{error, info, warn};

/// Liquidation Service
///
/// # Responsibilities
///
/// - Execute emergency liquidation of all positions
/// - Handle blind liquidation when prices unavailable (panic mode)
/// - Send liquidation orders to execution channel
pub struct LiquidationService {
    order_tx: Sender<Order>,
    portfolio_state_manager: Arc<PortfolioStateManager>,
    market_service: Arc<dyn MarketDataService>,
    order_retry_strategy: OrderRetryStrategy,
    spread_cache: Arc<SpreadCache>,
}

impl LiquidationService {
    /// Create a new LiquidationService
    pub fn new(
        order_tx: Sender<Order>,
        portfolio_state_manager: Arc<PortfolioStateManager>,
        market_service: Arc<dyn MarketDataService>,
        spread_cache: Arc<SpreadCache>,
    ) -> Self {
        Self {
            order_tx,
            portfolio_state_manager,
            market_service,
            order_retry_strategy: OrderRetryStrategy::new(RetryConfig::default()),
            spread_cache,
        }
    }

    /// Execute emergency liquidation of entire portfolio
    ///
    /// # Safety
    ///
    /// - Uses Market orders for guaranteed execution
    /// - Executes blind liquidation if prices unavailable (panic mode)
    /// - "Get me out at any price" is safer than staying in during a crash
    pub async fn liquidate_portfolio(
        &self,
        reason: &str,
        current_prices: &HashMap<String, Decimal>,
    ) {
        let snapshot = self.portfolio_state_manager.get_snapshot().await;

        info!(
            "LiquidationService: EMERGENCY LIQUIDATION TRIGGERED - Reason: {}",
            reason
        );

        // Pre-fetch REST prices as fallback for all positions
        let symbols: Vec<String> = snapshot.portfolio.positions.keys().cloned().collect();
        let fallback_prices = self
            .market_service
            .get_prices(symbols)
            .await
            .unwrap_or_default();

        for (symbol, position) in &snapshot.portfolio.positions {
            if position.quantity > Decimal::ZERO {
                // Get spread data for intelligent order placement
                let spread_data = self.spread_cache.get_spread_data(symbol);

                // Use input price, or fallback to REST price
                let current_price = current_prices
                    .get(symbol)
                    .cloned()
                    .or_else(|| fallback_prices.get(symbol).cloned())
                    .unwrap_or(Decimal::ZERO);

                // Panic mode (Blind Liquidation) if no price or price is zero
                let panic_mode = current_price <= Decimal::ZERO;

                if panic_mode {
                    warn!(
                        "LiquidationService: No price for {} (even REST fallback) - EXECUTING BLIND MARKET ORDER (Panic Mode)",
                        symbol
                    );
                }

                // Create smart liquidation order (Try Limit first if possible)
                let order = self.order_retry_strategy.create_liquidation_order(
                    symbol,
                    OrderSide::Sell,
                    position.quantity,
                    spread_data,
                    panic_mode,
                );

                warn!(
                    "LiquidationService: Placing EMERGENCY {:?} SELL for {} (Qty: {}) @ {}",
                    order.order_type, symbol, position.quantity, order.price
                );

                if let Err(e) = self.order_tx.send(order).await {
                    error!(
                        "LiquidationService: Failed to send liquidation order for {}: {}",
                        symbol, e
                    );
                }
            }
        }

        info!(
            "LiquidationService: Emergency liquidation orders placed. Trading HALTED. Manual review required."
        );
    }
}
