use crate::application::trailing_stops::StopState;
use crate::domain::types::OrderSide;
use tracing::info;

pub struct PositionManager {
    pub trailing_stop: StopState,
    pub pending_order: Option<OrderSide>,
    pub last_signal_time: i64,
}

impl PositionManager {
    pub fn new() -> Self {
        Self {
            trailing_stop: StopState::NoPosition,
            pending_order: None,
            last_signal_time: 0,
        }
    }

    pub fn ack_pending_orders(&mut self, has_position: bool, symbol: &str) {
        if let Some(pending) = self.pending_order {
            match pending {
                OrderSide::Buy => {
                    if has_position {
                        info!("PositionManager: Pending Buy for {} CONFIRMED.", symbol);
                        self.pending_order = None;
                    }
                }
                OrderSide::Sell => {
                    if !has_position {
                        info!("PositionManager: Pending Sell for {} CONFIRMED.", symbol);
                        self.pending_order = None;
                        self.trailing_stop.on_sell();
                    }
                }
            }
        }
    }

    pub fn check_trailing_stop(
        &mut self,
        symbol: &str,
        price: f64,
        atr: f64,
        multiplier: f64,
    ) -> Option<OrderSide> {
        if self.pending_order == Some(OrderSide::Sell) {
            return None;
        }

        if atr > 0.0 {
            if let Some(trigger) = self.trailing_stop.on_price_update(price, atr, multiplier) {
                info!(
                    "PositionManager: Trailing stop HIT for {} at {:.2} (Stop: {:.2}, Entry: {:.2})",
                    symbol, trigger.exit, trigger.stop, trigger.entry
                );
                return Some(OrderSide::Sell);
            }
        }
        None
    }
}
