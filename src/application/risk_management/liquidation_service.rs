//! Liquidation Service
//!
//! Handles emergency portfolio liquidation during circuit breaker events.
//! Extracted from RiskManager to follow Single Responsibility Principle.

use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::domain::trading::types::{Order, OrderSide, OrderType};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{error, info, warn};
use uuid::Uuid;

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
}

impl LiquidationService {
    /// Create a new LiquidationService
    pub fn new(
        order_tx: Sender<Order>,
        portfolio_state_manager: Arc<PortfolioStateManager>,
    ) -> Self {
        Self {
            order_tx,
            portfolio_state_manager,
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

        for (symbol, position) in &snapshot.portfolio.positions {
            if position.quantity > Decimal::ZERO {
                let current_price = current_prices.get(symbol).cloned().unwrap_or(Decimal::ZERO);

                // CRITICAL SAFETY: Blind liquidation if price unavailable
                if current_price <= Decimal::ZERO {
                    warn!(
                        "LiquidationService: No price for {} - EXECUTING BLIND MARKET ORDER (Panic Mode)",
                        symbol
                    );
                }

                // Use Market orders for emergency liquidation
                let order = Order {
                    id: Uuid::new_v4().to_string(),
                    symbol: symbol.clone(),
                    side: OrderSide::Sell,
                    price: Decimal::ZERO, // Market order ignores price
                    quantity: position.quantity,
                    order_type: OrderType::Market,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };

                warn!(
                    "LiquidationService: Placing EMERGENCY MARKET SELL for {} (Qty: {})",
                    symbol, position.quantity
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
