//! Hard Stop Manager
//!
//! Provides per-trade loss limits to prevent extreme drawdowns.
//! If a position's unrealized loss exceeds the configured threshold,
//! a forced exit signal is generated.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::warn;

/// Configuration for hard stop loss limits
#[derive(Debug, Clone)]
pub struct HardStopConfig {
    /// Maximum loss per trade as a negative percentage (e.g., -0.05 = -5%)
    pub max_loss_pct: Decimal,
}

impl Default for HardStopConfig {
    fn default() -> Self {
        Self {
            max_loss_pct: dec!(-0.05), // -5% default
        }
    }
}

/// Manager for enforcing hard stop-loss limits on positions
pub struct HardStopManager {
    config: HardStopConfig,
}

impl HardStopManager {
    pub fn new(max_loss_pct: Decimal) -> Self {
        Self {
            config: HardStopConfig { max_loss_pct },
        }
    }

    /// Check if a position should be force-exited due to exceeding loss threshold
    ///
    /// # Arguments
    /// * `entry_price` - The price at which the position was entered
    /// * `current_price` - The current market price
    ///
    /// # Returns
    /// * `true` if the position should be force-exited (loss exceeds threshold)
    /// * `false` if the position is within acceptable loss limits
    pub fn should_force_exit(&self, entry_price: Decimal, current_price: Decimal) -> bool {
        if entry_price <= Decimal::ZERO {
            return false;
        }

        let pnl_pct = (current_price - entry_price) / entry_price;

        if pnl_pct < self.config.max_loss_pct {
            warn!(
                "HardStop: Position loss {}% exceeds threshold {}%. Forcing exit.",
                pnl_pct * dec!(100.0),
                self.config.max_loss_pct * dec!(100.0)
            );
            return true;
        }

        false
    }

    /// Get the configured maximum loss percentage
    pub fn max_loss_pct(&self) -> Decimal {
        self.config.max_loss_pct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_within_threshold_no_exit() {
        let manager = HardStopManager::new(dec!(-0.05)); // -5%

        // -3% loss, within threshold
        let entry = dec!(100);
        let current = dec!(97);

        assert!(!manager.should_force_exit(entry, current));
    }

    #[test]
    fn test_exceeds_threshold_force_exit() {
        let manager = HardStopManager::new(dec!(-0.05)); // -5%

        // -6% loss, exceeds threshold
        let entry = dec!(100);
        let current = dec!(94);

        assert!(manager.should_force_exit(entry, current));
    }

    #[test]
    fn test_profit_no_exit() {
        let manager = HardStopManager::new(dec!(-0.05));

        // +10% profit
        let entry = dec!(100);
        let current = dec!(110);

        assert!(!manager.should_force_exit(entry, current));
    }

    #[test]
    fn test_exact_threshold_no_exit() {
        let manager = HardStopManager::new(dec!(-0.05)); // -5%

        // Exactly -5% loss
        let entry = dec!(100);
        let current = dec!(95);

        // At exactly the threshold, we don't force exit (only when exceeded)
        assert!(!manager.should_force_exit(entry, current));
    }
}
