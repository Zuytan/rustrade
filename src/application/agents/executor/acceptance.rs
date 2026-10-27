//! Post-submission path of orders accepted by the broker.

use super::{ActiveTrailingStop, Executor};
use crate::application::monitoring::connection_health_service::ConnectionStatus;
use crate::domain::trading::portfolio::Position;
use crate::domain::trading::types::{Order, OrderSide};
use tracing::{error, info, instrument, warn};

impl Executor {
    /// Post-submission path for an order the broker accepted: fees, optimistic portfolio
    /// update, persisted as New, retry tracking (if `track_for_retry`) and initial
    /// trailing stop.
    pub(super) async fn on_order_accepted(&self, order: &Order, track_for_retry: bool) {
        // 2. Retrieve real broker fees (non-blocking, graceful fallback)
        let real_fees = match self.execution_service.get_order_fees(&order.id).await {
            Ok(fees) => {
                if let Some(ref f) = fees {
                    info!("Executor: Real broker fees for {}: ${}", order.id, f);
                }
                fees
            }
            Err(e) => {
                warn!(
                    "Executor: Fee retrieval failed for {}: {}, using model estimate",
                    order.id, e
                );
                None
            }
        };

        // 3. Update Internal State (Optimistic) with real or estimated fees
        self.update_portfolio(order, false, real_fees).await;
        info!("Executor: Order {} sent to exchange.", order.id);

        // Update persisted status to 'New' (now officially on the exchange)
        if let Some(repo) = &self.repository {
            let mut submitted_order = order.clone();
            submitted_order.status = crate::domain::trading::types::OrderStatus::New;
            if let Err(e) = repo.save(&submitted_order).await {
                error!(
                    "Executor: Failed to persist submitted order {} as New: {}",
                    order.id, e
                );
            }
        }

        // Broadcast Online if it was offline
        self.health_service
            .set_execution_status(ConnectionStatus::Online, None)
            .await;

        // Track for retry monitoring if applicable (an order already over is not tracked)
        if track_for_retry {
            self.order_monitor.track_order(order.clone()).await;
        }

        // 4. Submit initial trailing stop if stop_loss is provided for Buy orders
        if order.side == OrderSide::Buy
            && let Some(sl_price) = order.stop_loss
        {
            let trailing_distance = (order.price - sl_price).abs();
            if trailing_distance > rust_decimal::Decimal::ZERO {
                let stop_id = uuid::Uuid::new_v4().to_string();
                let stop_order = Order {
                    id: stop_id.clone(),
                    symbol: order.symbol.clone(),
                    side: OrderSide::Sell,
                    price: sl_price,
                    quantity: order.quantity,
                    order_type: crate::domain::trading::types::OrderType::Stop,
                    status: crate::domain::trading::types::OrderStatus::Pending,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    correlation_id: order.correlation_id.clone(),
                    stop_loss: None,
                };

                let exec = self.execution_service.clone();
                let stop_order_clone = stop_order.clone();
                tokio::spawn(async move {
                    if let Err(e) = exec.execute(&stop_order_clone).await {
                        error!(
                            "Executor: Failed to place initial trailing stop order: {}",
                            e
                        );
                    }
                });

                let mut active_stops = self.active_trailing_stops.write().await;
                active_stops.insert(
                    order.symbol.clone(),
                    ActiveTrailingStop {
                        stop_order_id: stop_id,
                        stop_state:
                            crate::application::risk_management::trailing_stops::StopState::on_buy(
                                order.price,
                                trailing_distance,
                                rust_decimal::Decimal::ONE,
                            ),
                        quantity: order.quantity,
                        trailing_distance,
                        correlation_id: order.correlation_id.clone(),
                    },
                );
                info!(
                    "Executor: Initialized trailing stop for {} at stop price {}",
                    order.symbol, sl_price
                );
            }
        }
    }

    #[instrument(skip(self, order), fields(symbol = %order.symbol, side = ?order.side))]
    pub(super) async fn update_portfolio(
        &self,
        order: &Order,
        is_reversal: bool,
        real_fees: Option<rust_decimal::Decimal>,
    ) {
        let mut portfolio =
            match tokio::time::timeout(std::time::Duration::from_secs(2), self.portfolio.write())
                .await
            {
                Ok(guard) => guard,
                Err(_) => {
                    error!("Executor: Deadlock detected acquiring Portfolio write lock");
                    return;
                }
            };

        let cost = order.price * order.quantity;

        // Use real broker fees if available, otherwise fall back to model estimate
        let fees = if let Some(broker_fees) = real_fees {
            info!(
                "Executor: Using real broker fees: ${} for order {}",
                broker_fees, order.id
            );
            broker_fees
        } else {
            let trade_cost = self
                .fee_model
                .calculate_cost(order.quantity, order.price, order.side);
            let estimated = trade_cost.total_impact;
            info!(
                "Executor: No broker fees, using model estimate: ${} for order {}",
                estimated, order.id
            );
            estimated
        };

        match order.side {
            OrderSide::Buy => {
                // If reversal (Buy), we ADD cash and fees back. If normal (Buy), we SUBTRACT cost + fees.
                if is_reversal {
                    portfolio.cash += cost + fees;
                } else {
                    portfolio.cash -= cost + fees;
                }

                let position =
                    portfolio
                        .positions
                        .entry(order.symbol.clone())
                        .or_insert(Position {
                            symbol: order.symbol.clone(),
                            quantity: rust_decimal::Decimal::ZERO,
                            average_price: rust_decimal::Decimal::ZERO,
                        });

                // Update position logic is complex for reversal of average price
                // For simplicity in this fix, we primarily care about Quantity and Cash
                if is_reversal {
                    position.quantity -= order.quantity;
                    // NOTE: Average price reversal is lossy if we don't store history.
                    // Accepting this limitation for "blind" optimistic updates.
                } else {
                    let total_val = (position.quantity * position.average_price) + cost;
                    let new_qty = position.quantity + order.quantity;
                    if !new_qty.is_zero() {
                        position.average_price = total_val / new_qty;
                    }
                    position.quantity = new_qty;
                }
            }
            OrderSide::Sell => {
                if is_reversal {
                    portfolio.cash -= cost - fees;
                    if let Some(position) = portfolio.positions.get_mut(&order.symbol) {
                        position.quantity += order.quantity;
                    }
                } else {
                    portfolio.cash += cost - fees;
                    if let Some(position) = portfolio.positions.get_mut(&order.symbol) {
                        position.quantity -= order.quantity;
                    }
                }
            }
        }
    }
}
