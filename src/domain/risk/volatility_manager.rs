use std::collections::VecDeque;

/// Configuration for the Volatility Manager
#[derive(Debug, Clone)]
pub struct VolatilityConfig {
    /// Number of periods to calculate average volatility (e.g., 20)
    pub lookback_period: usize,
    /// Multiplier to dampen or amplify the effect (default 1.0)
    pub scaling_factor: f64,
    /// Maximum multiplier cap (e.g., 1.5x size in low vol)
    pub max_multiplier: f64,
    /// Minimum multiplier floor (e.g., 0.5x size in high vol)
    pub min_multiplier: f64,
}

impl Default for VolatilityConfig {
    fn default() -> Self {
        Self {
            lookback_period: 20,
            scaling_factor: 1.0,
            max_multiplier: 1.5,
            min_multiplier: 0.5,
        }
    }
}

/// Service to adjust risk/position sizing based on market volatility
#[derive(Debug)]
pub struct VolatilityManager {
    config: VolatilityConfig,
    /// Sliding window of ATR values or daily ranges
    history: VecDeque<f64>,
}

impl VolatilityManager {
    pub fn new(config: VolatilityConfig) -> Self {
        Self {
            config,
            history: VecDeque::new(),
        }
    }

    /// Update with new volatility measurement (e.g., today's TR or current ATR)
    pub fn update(&mut self, volatility_value: f64) {
        if volatility_value <= 0.0 {
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
    pub fn calculate_multiplier(&self, current_volatility: f64) -> f64 {
        if self.history.is_empty() {
            return 1.0; // No history, neutral multiplier
        }

        let sum: f64 = self.history.iter().sum();
        let avg_volatility = sum / self.history.len() as f64;

        if current_volatility <= 0.0 || avg_volatility <= 0.0 {
            return 1.0;
        }

        // Ratio: Average / Current
        // Example with ATR:
        // Avg ATR = 2.0, Current ATR = 4.0 (High Vol) -> Ratio = 0.5 (Half size)
        // Avg ATR = 2.0, Current ATR = 1.0 (Low Vol) -> Ratio = 2.0 (Double size)
        let raw_ratio = avg_volatility / current_volatility;
        
        // Apply scaling factor (dampening)
        // If scale is 1.0, we use raw ratio.
        // If scale is 0.5, we move closer to 1.0.
        // Logarithmic scaling might be safer but linear is simple for now.
        // Let's just clamp the raw ratio.
        
        let multiplier = raw_ratio * self.config.scaling_factor;

        // Clamp between min and max
        multiplier.clamp(self.config.min_multiplier, self.config.max_multiplier)
    }

    /// Helper to get current average volatility
    pub fn get_average_volatility(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        self.history.iter().sum::<f64>() / self.history.len() as f64
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
            manager.update(1.0);
        }

        // Current volatility spikes to 4.0
        let multiplier = manager.calculate_multiplier(4.0);
        
        // Expected: 1.0 / 4.0 = 0.25 -> clamped to min 0.5
        assert_eq!(multiplier, 0.5);
    }

    #[test]
    fn test_low_volatility_increases_multiplier() {
        let config = VolatilityConfig::default(); // Min 0.5, Max 1.5
        let mut manager = VolatilityManager::new(config);

        // Fill history with high volatility (avg = 4.0)
        for _ in 0..20 {
            manager.update(4.0);
        }

        // Current volatility drops to 2.0
        let multiplier = manager.calculate_multiplier(2.0);
        
        // Expected: 4.0 / 2.0 = 2.0 -> clamped to max 1.5
        assert_eq!(multiplier, 1.5);
    }

    #[test]
    fn test_normal_volatility() {
        let config = VolatilityConfig::default();
        let mut manager = VolatilityManager::new(config);

        // Fill history with avg = 2.0
        for _ in 0..20 {
            manager.update(2.0);
        }

        // Current = 2.0
        let multiplier = manager.calculate_multiplier(2.0);
        
        // Expected: 1.0
        assert!((multiplier - 1.0).abs() < 0.001);
    }
}
