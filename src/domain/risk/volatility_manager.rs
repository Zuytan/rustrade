use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::VecDeque;

/// Configuration for the Volatility Manager
#[derive(Debug, Clone)]
pub struct VolatilityConfig {
    /// Number of periods to calculate average volatility (e.g., 20)
    pub lookback_period: usize,
    /// Multiplier to dampen or amplify the effect (default 1.0)
    pub scaling_factor: Decimal,
    /// Maximum multiplier cap (e.g., 1.5x size in low vol)
    pub max_multiplier: Decimal,
    /// Minimum multiplier floor (e.g., 0.5x size in high vol)
    pub min_multiplier: Decimal,
}

impl Default for VolatilityConfig {
    fn default() -> Self {
        Self {
            lookback_period: 20,
            scaling_factor: Decimal::ONE,
            max_multiplier: dec!(1.5),
            min_multiplier: dec!(0.5),
        }
    }
}

/// Service to adjust risk/position sizing based on market volatility
#[derive(Debug)]
pub struct VolatilityManager {
    config: VolatilityConfig,
    /// Sliding window of ATR values or daily ranges
    history: VecDeque<Decimal>,
}

impl VolatilityManager {
    pub fn new(config: VolatilityConfig) -> Self {
        Self {
            config,
            history: VecDeque::new(),
        }
    }

    /// Update with new volatility measurement (e.g., today's TR or current ATR)
    pub fn update(&mut self, volatility_value: Decimal) {
        if volatility_value <= Decimal::ZERO {
            return;
        }

        self.history.push_back(volatility_value);
        if self.history.len() > self.config.lookback_period {
            self.history.pop_front();
        }
    }

    /// Calculate the Position Size Multiplier
    ///
    /// Logic:
    /// - If current volatility > average volatility -> Low Multiplier (< 1.0)
    /// - If current volatility < average volatility -> High Multiplier (> 1.0)
    ///
    /// Formula: Multiplier = (Baseline / Current) * Scale
    pub fn calculate_multiplier(&self, current_volatility: Decimal) -> Decimal {
        if self.history.is_empty() {
            return Decimal::ONE; // No history, neutral multiplier
        }

        let avg_volatility = self.get_average_volatility();

        if current_volatility <= Decimal::ZERO || avg_volatility <= Decimal::ZERO {
            return Decimal::ONE;
        }

        // Ratio: Average / Current
        let raw_ratio = avg_volatility / current_volatility;

        let multiplier = raw_ratio * self.config.scaling_factor;

        // Clamp between min and max
        multiplier.clamp(self.config.min_multiplier, self.config.max_multiplier)
    }

    /// Helper to get current average volatility
    pub fn get_average_volatility(&self) -> Decimal {
        if self.history.is_empty() {
            return Decimal::ZERO;
        }
        let sum: Decimal = self.history.iter().sum();
        sum / Decimal::from(self.history.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_volatility_reduces_multiplier() {
        let config = VolatilityConfig::default(); // Min 0.5, Max 1.5
        let mut manager = VolatilityManager::new(config);

        // Fill history with low volatility (avg = 1.0)
        for _ in 0..20 {
            manager.update(Decimal::ONE);
        }

        // Current volatility spikes to 4.0
        let multiplier = manager.calculate_multiplier(dec!(4.0));

        // Expected: 1.0 / 4.0 = 0.25 -> clamped to min 0.5
        assert_eq!(multiplier, dec!(0.5));
    }

    #[test]
    fn test_low_volatility_increases_multiplier() {
        let config = VolatilityConfig::default(); // Min 0.5, Max 1.5
        let mut manager = VolatilityManager::new(config);

        // Fill history with high volatility (avg = 4.0)
        for _ in 0..20 {
            manager.update(dec!(4.0));
        }

        // Current volatility drops to 2.0
        let multiplier = manager.calculate_multiplier(dec!(2.0));

        // Expected: 4.0 / 2.0 = 2.0 -> clamped to max 1.5
        assert_eq!(multiplier, dec!(1.5));
    }

    #[test]
    fn test_normal_volatility() {
        let config = VolatilityConfig::default();
        let mut manager = VolatilityManager::new(config);

        // Fill history with avg = 2.0
        for _ in 0..20 {
            manager.update(dec!(2.0));
        }

        // Current = 2.0
        let multiplier = manager.calculate_multiplier(dec!(2.0));

        // Expected: 1.0
        assert_eq!(multiplier, Decimal::ONE);
    }
}
