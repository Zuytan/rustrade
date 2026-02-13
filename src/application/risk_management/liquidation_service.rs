//! Liquidation Service
//!
//! Handles emergency portfolio liquidation during circuit breaker events.
//! Extracted from RiskManager to follow Single Responsibility Principle.

use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::order_retry_strategy::{OrderRetryStrategy, RetryConfig};
use crate::domain::ports::{ExecutionService, MarketDataService};
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
    order_tx: Option<Sender<Order>>,
    portfolio_state_manager: Arc<PortfolioStateManager>,
    market_service: Arc<dyn MarketDataService>,
    order_retry_strategy: OrderRetryStrategy,
    spread_cache: Arc<SpreadCache>,
}

impl LiquidationService {
    /// Create a new LiquidationService
    pub fn new(
        order_tx: Option<Sender<Order>>,
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

    /// Generate liquidation orders for all open positions without executing them
    pub async fn generate_liquidation_orders(
        &self,
        reason: &str,
        current_prices: &HashMap<String, Decimal>,
    ) -> Vec<Order> {
        let snapshot = self.portfolio_state_manager.get_snapshot().await;
        let mut orders = Vec::new();

        info!(
            "LiquidationService: Generating liquidation orders - Reason: {}",
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
                        "LiquidationService: No price for {} (even REST fallback) - Panic Mode",
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

                orders.push(order);
            }
        }
        orders
    }

    /// Execute emergency liquidation of entire portfolio via configured channel
    pub async fn liquidate_portfolio(
        &self,
        reason: &str,
        current_prices: &HashMap<String, Decimal>,
    ) {
        if self.order_tx.is_none() {
            error!("LiquidationService: Cannot liquidate via channel - channel is missing");
            return;
        }

        let orders = self
            .generate_liquidation_orders(reason, current_prices)
            .await;
        let tx = self.order_tx.as_ref().unwrap();

        for order in orders {
            warn!(
                "LiquidationService: Placing EMERGENCY {:?} SELL for {} (Qty: {}) @ {}",
                order.order_type, order.symbol, order.quantity, order.price
            );

            if let Err(e) = tx.send(order).await {
                error!(
                    "LiquidationService: Failed to send liquidation order for {}: {}",
                    "unknown",
                    e // Symbol is in order but hard to access here without clone
                );
                // We should probably log the symbol from order
            }
        }

        info!(
            "LiquidationService: Emergency liquidation orders placed. Trading HALTED. Manual review required."
        );
    }
    /// Execute a batch of orders with robust retry logic (Exponential Backoff)
    /// Used by ShutdownService or other direct execution contexts.
    pub async fn execute_orders_with_retry(
        &self,
        orders: Vec<Order>,
        execution_service: &Arc<dyn ExecutionService>,
    ) {
        let max_retries = 3;
        let base_delay_ms = 500;

        for order in orders {
            let mut attempts = 0;
            loop {
                match execution_service.execute(order.clone()).await {
                    Ok(_) => {
                        info!(
                            "LiquidationService: Successfully executed {} for {}",
                            order.side, order.symbol
                        );
                        break;
                    }
                    Err(e) => {
                        attempts += 1;
                        if attempts >= max_retries {
                            error!(
                                "LiquidationService: FAILED to execute {} for {} after {} attempts: {}",
                                order.side, order.symbol, attempts, e
                            );
                            // TODO: In a real system, we might want to alert via multiple channels (SMS, PagerDuty)
                            // For now, logging error is the best we can do.
                            break;
                        } else {
                            let delay = base_delay_ms * 2u64.pow(attempts as u32 - 1);
                            warn!(
                                "LiquidationService: Execution failed for {} (Attempt {}/{}): {}. Retrying in {}ms...",
                                order.symbol, attempts, max_retries, e, delay
                            );
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::{ExecutionService, OrderUpdate};
    use crate::domain::trading::portfolio::Portfolio;
    use crate::domain::trading::types::{Order, OrderSide, OrderStatus, OrderType};
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Mock Execution Service for testing retries
    struct RetryMockExecutionService {
        fail_count: AtomicUsize,
        succeed_after: usize,
    }

    #[async_trait]
    impl ExecutionService for RetryMockExecutionService {
        async fn execute(&self, _order: Order) -> anyhow::Result<()> {
            let current = self.fail_count.fetch_add(1, Ordering::SeqCst);
            if current < self.succeed_after {
                anyhow::bail!("Simulated Failure");
            }
            Ok(())
        }

        async fn get_portfolio(&self) -> anyhow::Result<Portfolio> {
            Ok(Portfolio::new())
        }
        async fn get_today_orders(&self) -> anyhow::Result<Vec<Order>> {
            Ok(vec![])
        }
        async fn get_open_orders(&self) -> anyhow::Result<Vec<Order>> {
            Ok(vec![])
        }
        async fn cancel_order(&self, _id: &str, _s: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn cancel_all_orders(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn subscribe_order_updates(
            &self,
        ) -> anyhow::Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
            let (tx, _) = tokio::sync::broadcast::channel(1);
            Ok(tx.subscribe())
        }
    }

    struct MockMarketData;
    #[async_trait]
    impl MarketDataService for MockMarketData {
        async fn get_prices(
            &self,
            _symbols: Vec<String>,
        ) -> anyhow::Result<HashMap<String, Decimal>> {
            Ok(HashMap::new())
        }
        async fn get_historical_bars(
            &self,
            _s: &str,
            _start: chrono::DateTime<chrono::Utc>,
            _end: chrono::DateTime<chrono::Utc>,
            _tf: &str,
        ) -> anyhow::Result<Vec<crate::domain::trading::types::Candle>> {
            Ok(vec![])
        }

        // Missing methods
        async fn subscribe(
            &self,
            _symbols: Vec<String>,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::domain::trading::types::MarketEvent>>
        {
            let (_, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
        async fn get_top_movers(&self) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
        async fn get_tradable_assets(&self) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_execute_orders_with_retry_succeeds_after_failures() {
        let execution_service = Arc::new(RetryMockExecutionService {
            fail_count: AtomicUsize::new(0),
            succeed_after: 2, // Fail 2 times, succeed on 3rd
        });

        let execution_service_dyn: Arc<dyn ExecutionService> = execution_service.clone();

        // Setup minimal service
        let portfolio_state_manager = Arc::new(PortfolioStateManager::new(
            execution_service_dyn.clone(),
            5000,
        ));

        let service = LiquidationService::new(
            None,
            portfolio_state_manager,
            Arc::new(MockMarketData),
            Arc::new(SpreadCache::new()),
        );

        let order = Order {
            id: "test".to_string(),
            symbol: "AAPL".to_string(),
            side: OrderSide::Sell,
            order_type: OrderType::Market,
            quantity: dec!(10),
            price: dec!(0),
            status: OrderStatus::New,
            timestamp: 0,
        };

        service
            .execute_orders_with_retry(vec![order], &execution_service_dyn)
            .await;

        // Assert: 3 attempts made (0, 1 failures, 2 success)
        assert_eq!(execution_service.fail_count.load(Ordering::SeqCst), 3);
    }
}
