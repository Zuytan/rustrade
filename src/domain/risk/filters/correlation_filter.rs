use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Configuration for correlation-based diversification
#[derive(Debug, Clone)]
pub struct CorrelationFilterConfig {
    /// Maximum allowed correlation with any existing position (e.g., 0.85)
    pub max_correlation_threshold: Decimal,
}

impl Default for CorrelationFilterConfig {
    fn default() -> Self {
        Self {
            max_correlation_threshold: dec!(0.85),
        }
    }
}

use crate::domain::risk::filters::validator_trait::{
    RiskValidator, ValidationContext, ValidationResult,
};
use crate::domain::trading::portfolio::Position;
use crate::domain::trading::types::OrderSide;
use async_trait::async_trait;
use std::collections::HashMap;

pub struct CorrelationFilter {
    config: CorrelationFilterConfig,
}

impl CorrelationFilter {
    pub fn new(config: CorrelationFilterConfig) -> Self {
        Self { config }
    }

    pub fn check_correlation(
        target_symbol: &str,
        positions: &HashMap<String, Position>,
        correlation_matrix: &HashMap<(String, String), Decimal>,
        config: &CorrelationFilterConfig,
    ) -> Result<(), String> {
        if positions.is_empty() {
            return Ok(());
        }

        for existing_symbol in positions.keys() {
            if existing_symbol == target_symbol {
                continue;
            }

            let corr = correlation_matrix
                .get(&(target_symbol.to_string(), existing_symbol.clone()))
                .or_else(|| {
                    correlation_matrix.get(&(existing_symbol.clone(), target_symbol.to_string()))
                })
                .cloned()
                .unwrap_or(Decimal::ZERO);

            if corr > config.max_correlation_threshold {
                return Err(format!(
                    "Correlation too high between {} and existing position {} ({} > {})",
                    target_symbol, existing_symbol, corr, config.max_correlation_threshold
                ));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl RiskValidator for CorrelationFilter {
    fn name(&self) -> &str {
        "CorrelationFilter"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        // Only validate Buy orders
        if !matches!(ctx.proposal.side, OrderSide::Buy) {
            return ValidationResult::Approve;
        }

        // Need correlation matrix
        let matrix = match ctx.correlation_matrix {
            Some(m) => m,
            None => return ValidationResult::Approve, // No data, can't validate
        };

        match Self::check_correlation(
            &ctx.proposal.symbol,
            &ctx.portfolio.positions,
            matrix,
            &self.config,
        ) {
            Ok(_) => ValidationResult::Approve,
            Err(e) => ValidationResult::Reject(e),
        }
    }

    fn priority(&self) -> u8 {
        35 // After Sector Exposure, before Sentiment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_block_high_correlation() {
        let mut positions = HashMap::new();
        positions.insert(
            "BTC/USD".to_string(),
            Position {
                symbol: "BTC/USD".to_string(),
                quantity: dec!(1),
                average_price: dec!(50000),
            },
        );

        let mut matrix = HashMap::new();
        matrix.insert(("ETH/USD".to_string(), "BTC/USD".to_string()), dec!(0.95));

        let config = CorrelationFilterConfig {
            max_correlation_threshold: dec!(0.85),
        };

        let result = CorrelationFilter::check_correlation("ETH/USD", &positions, &matrix, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Correlation too high"));
    }

    #[test]
    fn test_allow_low_correlation() {
        let mut positions = HashMap::new();
        positions.insert(
            "BTC/USD".to_string(),
            Position {
                symbol: "BTC/USD".to_string(),
                quantity: dec!(1),
                average_price: dec!(50000),
            },
        );

        let mut matrix = HashMap::new();
        matrix.insert(("GLD".to_string(), "BTC/USD".to_string()), dec!(0.10));

        let config = CorrelationFilterConfig::default();

        let result = CorrelationFilter::check_correlation("GLD", &positions, &matrix, &config);
        assert!(result.is_ok());
    }
}
