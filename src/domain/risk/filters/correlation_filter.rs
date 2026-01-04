use std::collections::HashMap;
use crate::domain::trading::portfolio::Position;

/// Configuration for correlation-based diversification
#[derive(Debug, Clone)]
pub struct CorrelationFilterConfig {
    /// Maximum allowed correlation with any existing position (e.g., 0.85)
    pub max_correlation_threshold: f64,
}

impl Default for CorrelationFilterConfig {
    fn default() -> Self {
        Self {
            max_correlation_threshold: 0.85,
        }
    }
}

pub struct CorrelationFilter;

impl CorrelationFilter {
    /// Checks if a new trade (Buy) should be blocked due to high correlation with existing positions.
    /// Returns Err(message) if blocked, Ok(()) otherwise.
    pub fn check_correlation(
        target_symbol: &str,
        positions: &HashMap<String, Position>,
        correlation_matrix: &HashMap<(String, String), f64>,
        config: &CorrelationFilterConfig,
    ) -> Result<(), String> {
        // If no positions, correlation check is always fine
        if positions.is_empty() {
            return Ok(());
        }

        for existing_symbol in positions.keys() {
            // Self-correlation is 1.0, but we only care about OTHER symbols
            if existing_symbol == target_symbol {
                continue;
            }

            // Get correlation from matrix (try both combinations as matrix might be upper/lower triangle only, 
            // though CorrelationService fills both)
            let corr = correlation_matrix.get(&(target_symbol.to_string(), existing_symbol.clone()))
                .or_else(|| correlation_matrix.get(&(existing_symbol.clone(), target_symbol.to_string())))
                .cloned()
                .unwrap_or(0.0); // Default to 0 if no data

            if corr > config.max_correlation_threshold {
                return Err(format!(
                    "Correlation too high between {} and existing position {} ({:.2} > {:.2})",
                    target_symbol,
                    existing_symbol,
                    corr,
                    config.max_correlation_threshold
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_block_high_correlation() {
        let mut positions = HashMap::new();
        positions.insert("BTC/USD".to_string(), Position {
            symbol: "BTC/USD".to_string(),
            quantity: dec!(1),
            average_price: dec!(50000),
        });

        let mut matrix = HashMap::new();
        matrix.insert(("ETH/USD".to_string(), "BTC/USD".to_string()), 0.95);

        let config = CorrelationFilterConfig { max_correlation_threshold: 0.85 };
        
        let result = CorrelationFilter::check_correlation("ETH/USD", &positions, &matrix, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Correlation too high"));
    }

    #[test]
    fn test_allow_low_correlation() {
        let mut positions = HashMap::new();
        positions.insert("BTC/USD".to_string(), Position {
            symbol: "BTC/USD".to_string(),
            quantity: dec!(1),
            average_price: dec!(50000),
        });

        let mut matrix = HashMap::new();
        matrix.insert(("GLD".to_string(), "BTC/USD".to_string()), 0.10);

        let config = CorrelationFilterConfig::default();
        
        let result = CorrelationFilter::check_correlation("GLD", &positions, &matrix, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_allow_missing_data() {
        let mut positions = HashMap::new();
        positions.insert("BTC/USD".to_string(), Position {
            symbol: "BTC/USD".to_string(),
            quantity: dec!(1),
            average_price: dec!(50000),
        });

        let matrix = HashMap::new(); // Empty matrix

        let config = CorrelationFilterConfig::default();
        
        let result = CorrelationFilter::check_correlation("UNKNOWN", &positions, &matrix, &config);
        assert!(result.is_ok()); // Should not block if we don't know the correlation
    }
}
