//! Trailing Stop State Machine
//!
//! This module implements the State Pattern for trailing stop loss management.
//! It provides a clean abstraction for tracking position entry, peak prices,
//! and automatic stop loss triggers based on ATR (Average True Range).
//!
//! # Design
//!
//! The `StopState` enum represents three distinct states:
//! - `NoPosition`: No active position to protect
//! - `ActiveStop`: Trailing stop is active and tracking price movements
//! - `Triggered`: Stop loss was hit
//!
//! # Example
//!
//! ```rust,no_run
//! use rustrade::application::risk_management::trailing_stops::StopState;
//! use rust_decimal::Decimal;
//!
//! let mut stop = StopState::on_buy(
//!     Decimal::from(100),
//!     Decimal::from(2),
//!     Decimal::from(3)
//! ); // price=100, ATR=2, multiplier=3
//!
//! // Price rises to 110
//! let trigger = stop.on_price_update(Decimal::from(110), Decimal::from(2), Decimal::from(3));
//! assert!(trigger.is_none()); // Stop raised, not triggered
//!
//! // Price drops below stop
//! let trigger = stop.on_price_update(Decimal::from(103), Decimal::from(2), Decimal::from(3));
//! assert!(trigger.is_some()); // Stop triggered at 104
//! ```

use rust_decimal::Decimal;

/// State machine for trailing stop loss management
#[derive(Debug, Clone, PartialEq)]
pub enum StopState {
    /// No active position
    NoPosition,
    /// Active trailing stop with position
    ActiveStop {
        entry_price: Decimal,
        peak_price: Decimal,
        stop_price: Decimal,
        atr: Decimal,
    },
    /// Stop was triggered
    Triggered {
        entry_price: Decimal,
        exit_price: Decimal,
        stop_price: Decimal,
    },
}

/// Event emitted when a trailing stop is triggered
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub entry: Decimal,
    pub exit: Decimal,
    pub stop: Decimal,
}

impl StopState {
    /// Create a new active stop when buying
    pub fn on_buy(price: Decimal, atr: Decimal, multiplier: Decimal) -> Self {
        let stop_price = price - (atr * multiplier);
        StopState::ActiveStop {
            entry_price: price,
            peak_price: price,
            stop_price,
            atr,
        }
    }

    /// Update stop on price movement
    /// Returns Some(TriggerEvent) if stop is hit
    pub fn on_price_update(
        &mut self,
        price: Decimal,
        atr: Decimal,
        multiplier: Decimal,
    ) -> Option<TriggerEvent> {
        match self {
            StopState::ActiveStop {
                entry_price,
                peak_price,
                stop_price,
                ..
            } => {
                // Update peak if new high
                if price > *peak_price {
                    *peak_price = price;
                    *stop_price = price - (atr * multiplier);
                    return None;
                }

                // Check if stop hit
                if price < *stop_price {
                    let trigger = TriggerEvent {
                        entry: *entry_price,
                        exit: price,
                        stop: *stop_price,
                    };
                    *self = StopState::Triggered {
                        entry_price: *entry_price,
                        exit_price: price,
                        stop_price: *stop_price,
                    };
                    return Some(trigger);
                }

                None
            }
            _ => None,
        }
    }

    /// Reset stop when selling
    pub fn on_sell(&mut self) {
        *self = StopState::NoPosition;
    }

    /// Check if stop is currently active
    pub fn is_active(&self) -> bool {
        matches!(self, StopState::ActiveStop { .. })
    }

    /// Get current stop price if active
    pub fn get_stop_price(&self) -> Option<Decimal> {
        match self {
            StopState::ActiveStop { stop_price, .. } => Some(*stop_price),
            _ => None,
        }
    }

    /// Get peak price if active
    pub fn get_peak_price(&self) -> Option<Decimal> {
        match self {
            StopState::ActiveStop { peak_price, .. } => Some(*peak_price),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_on_buy_creates_active_stop() {
        let stop = StopState::on_buy(Decimal::from(100), Decimal::from(2), Decimal::from(3));

        match stop {
            StopState::ActiveStop {
                entry_price,
                peak_price,
                stop_price,
                atr,
            } => {
                assert_eq!(entry_price, Decimal::from(100));
                assert_eq!(peak_price, Decimal::from(100));
                assert_eq!(stop_price, Decimal::from(94)); // 100 - (2 * 3)
                assert_eq!(atr, Decimal::from(2));
            }
            _ => panic!("Should be ActiveStop"),
        }
    }

    #[test]
    fn test_price_update_raises_stop() {
        let mut stop = StopState::on_buy(Decimal::from(100), Decimal::from(2), Decimal::from(3));
        let trigger = stop.on_price_update(Decimal::from(110), Decimal::from(2), Decimal::from(3));

        assert!(trigger.is_none());
        match stop {
            StopState::ActiveStop {
                peak_price,
                stop_price,
                ..
            } => {
                assert_eq!(peak_price, Decimal::from(110));
                assert_eq!(stop_price, Decimal::from(104)); // 110 - (2 * 3)
            }
            _ => panic!("Should still be ActiveStop"),
        }
    }

    #[test]
    fn test_price_update_no_change_when_below_peak() {
        let mut stop = StopState::on_buy(Decimal::from(100), Decimal::from(2), Decimal::from(3));
        stop.on_price_update(Decimal::from(110), Decimal::from(2), Decimal::from(3)); // Raise to 110

        // Price drops but not below stop
        let trigger = stop.on_price_update(Decimal::from(107), Decimal::from(2), Decimal::from(3));
        assert!(trigger.is_none());

        match stop {
            StopState::ActiveStop {
                peak_price,
                stop_price,
                ..
            } => {
                assert_eq!(peak_price, Decimal::from(110)); // Peak unchanged
                assert_eq!(stop_price, Decimal::from(104)); // Stop unchanged
            }
            _ => panic!("Should still be ActiveStop"),
        }
    }

    #[test]
    fn test_stop_triggered() {
        let mut stop = StopState::on_buy(Decimal::from(100), Decimal::from(2), Decimal::from(3));
        stop.on_price_update(Decimal::from(110), Decimal::from(2), Decimal::from(3)); // Raise to 110, stop at 104

        let trigger = stop.on_price_update(Decimal::from(103), Decimal::from(2), Decimal::from(3)); // Below stop (104)
        assert!(trigger.is_some());

        let event = trigger.unwrap();
        assert_eq!(event.entry, Decimal::from(100));
        assert_eq!(event.exit, Decimal::from(103));
        assert_eq!(event.stop, Decimal::from(104));

        assert!(matches!(stop, StopState::Triggered { .. }));
    }

    #[test]
    fn test_on_sell_resets() {
        let mut stop = StopState::on_buy(Decimal::from(100), Decimal::from(2), Decimal::from(3));
        stop.on_sell();
        assert!(matches!(stop, StopState::NoPosition));
    }

    #[test]
    fn test_is_active() {
        let mut stop = StopState::NoPosition;
        assert!(!stop.is_active());

        stop = StopState::on_buy(Decimal::from(100), Decimal::from(2), Decimal::from(3));
        assert!(stop.is_active());

        stop.on_sell();
        assert!(!stop.is_active());
    }

    #[test]
    fn test_get_stop_price() {
        let mut stop = StopState::NoPosition;
        assert_eq!(stop.get_stop_price(), None);

        stop = StopState::on_buy(Decimal::from(100), Decimal::from(2), Decimal::from(3));
        assert_eq!(stop.get_stop_price(), Some(Decimal::from(94)));
    }

    #[test]
    fn test_no_update_when_no_position() {
        let mut stop = StopState::NoPosition;
        let trigger = stop.on_price_update(Decimal::from(100), Decimal::from(2), Decimal::from(3));
        assert!(trigger.is_none());
        assert!(matches!(stop, StopState::NoPosition));
    }
}
