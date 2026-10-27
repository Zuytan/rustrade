//! Trailing stop updates driven by incoming candles.

use super::Executor;
use crate::domain::trading::types::{Order, OrderSide};
use tracing::{error, info, warn};

impl Executor {
    pub(super) async fn handle_candle(&self, candle: crate::domain::trading::types::Candle) {
        let mut active_stops = self.active_trailing_stops.write().await;
        if let Some(active) = active_stops.get_mut(&candle.symbol) {
            let old_stop_price = active.stop_state.get_stop_price();
            let trigger = active.stop_state.on_price_update(
                candle.close,
                active.trailing_distance,
                rust_decimal::Decimal::ONE,
            );

            if trigger.is_some() {
                info!(
                    "Executor: Trailing stop triggered locally for {} at price {}",
                    candle.symbol, candle.close
                );
                active_stops.remove(&candle.symbol);
            } else if let Some(new_stop_price) = active
                .stop_state
                .get_stop_price()
                .filter(|&p| Some(p) != old_stop_price)
            {
                info!(
                    "Executor: Trailing stop price updated for {} from {:?} to {}",
                    candle.symbol, old_stop_price, new_stop_price
                );

                let old_id = active.stop_order_id.clone();
                let symbol = candle.symbol.clone();
                let exec = self.execution_service.clone();

                let new_id = uuid::Uuid::new_v4().to_string();
                let qty = active.quantity;
                let correlation_id = active.correlation_id.clone();

                let new_stop_order = Order {
                    id: new_id.clone(),
                    symbol: symbol.clone(),
                    side: OrderSide::Sell,
                    price: new_stop_price,
                    quantity: qty,
                    order_type: crate::domain::trading::types::OrderType::Stop,
                    status: crate::domain::trading::types::OrderStatus::Pending,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    correlation_id,
                    stop_loss: None,
                };

                let new_id_spawn = new_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = exec.cancel_order(&old_id, &symbol).await {
                        warn!(
                            "Executor: Failed to cancel old trailing stop order {}: {}",
                            old_id, e
                        );
                    }
                    if let Err(e) = exec.execute(&new_stop_order).await {
                        error!(
                            "Executor: Failed to place updated trailing stop order {}: {}",
                            new_id_spawn, e
                        );
                    }
                });

                active.stop_order_id = new_id;
            }
        }
    }
}
