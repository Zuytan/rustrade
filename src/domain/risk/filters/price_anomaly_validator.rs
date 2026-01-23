use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use tracing::debug;

use crate::domain::risk::filters::validator_trait::{
    RiskValidator, ValidationContext, ValidationResult,
};
use crate::domain::trading::types::Candle;

/// Configuration for price anomaly detection
#[derive(Debug, Clone)]
pub struct PriceAnomalyConfig {
    /// Maximum allowed price deviation from SMA as percentage (e.g., 0.05 = 5%)
    pub max_deviation_pct: f64,

    /// Lookback period in candles for SMA calculation (default: 5 for 5-minute SMA with 1-min candles)
    pub lookback_candles: usize,
}

impl Default for PriceAnomalyConfig {
    fn default() -> Self {
        Self {
            max_deviation_pct: 0.05, // 5% threshold
            lookback_candles: 5,     // 5 candles = 5 minutes with 1-min timeframe
        }
    }
}

/// Validates that trade prices are not anomalously different from recent market prices
///
/// This validator protects against:
/// - Fat finger errors (typos in order entry)
/// - Flash crashes / flash rallies
/// - Stale or corrupted price feeds
///
/// It calculates a Simple Moving Average (SMA) of recent candle closes and rejects
/// trades if the proposal price deviates more than the configured threshold.
pub struct PriceAnomalyValidator {
    config: PriceAnomalyConfig,
}

impl PriceAnomalyValidator {
    pub fn new(config: PriceAnomalyConfig) -> Self {
        Self { config }
    }

    /// Calculate SMA from recent candles
    fn calculate_sma(&self, candles: &[Candle]) -> Option<Decimal> {
        if candles.is_empty() {
            return None;
        }

        let sum: Decimal = candles.iter().map(|c| c.close).sum();
        let count = Decimal::from(candles.len());

        if count > Decimal::ZERO {
            Some(sum / count)
        } else {
            None
        }
    }

    /// Check if price deviates too much from SMA
    fn check_price_deviation(&self, price: Decimal, sma: Decimal) -> Option<String> {
        if sma <= Decimal::ZERO {
            return None; // Cannot calculate deviation with zero or negative SMA
        }

        let deviation = ((price - sma) / sma).abs().to_f64().unwrap_or(0.0);

        if deviation > self.config.max_deviation_pct {
            return Some(format!(
                "Price anomaly detected: {:.2}% deviation from {}-candle SMA (limit: {:.2}%) [Price: {}, SMA: {}]",
                deviation * 100.0,
                self.config.lookback_candles,
                self.config.max_deviation_pct * 100.0,
                price,
                sma
            ));
        }

        None
    }
}

#[async_trait]
impl RiskValidator for PriceAnomalyValidator {
    fn name(&self) -> &str {
        "PriceAnomalyValidator"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        // If no candle data available, approve (fail-safe on startup)
        let candles = match ctx.recent_candles {
            Some(c) if !c.is_empty() => c,
            _ => {
                debug!(
                    "PriceAnomalyValidator: No candle data for {} - approving (fail-safe)",
                    ctx.proposal.symbol
                );
                return ValidationResult::Approve;
            }
        };

        // If insufficient data for SMA, approve
        if candles.len() < self.config.lookback_candles {
            debug!(
                "PriceAnomalyValidator: Insufficient data for {} ({} candles, need {}) - approving",
                ctx.proposal.symbol,
                candles.len(),
                self.config.lookback_candles
            );
            return ValidationResult::Approve;
        }

        // Take last N candles for SMA
        let recent_candles = &candles[candles.len().saturating_sub(self.config.lookback_candles)..];

        // Calculate SMA
        let sma = match self.calculate_sma(recent_candles) {
            Some(s) => s,
            None => {
                debug!(
                    "PriceAnomalyValidator: Failed to calculate SMA for {} - approving",
                    ctx.proposal.symbol
                );
                return ValidationResult::Approve;
            }
        };

        // Check deviation
        if let Some(reason) = self.check_price_deviation(ctx.proposal.price, sma) {
            return ValidationResult::Reject(reason);
        }

        ValidationResult::Approve
    }

    fn priority(&self) -> u8 {
        10 // After circuit breakers (1), same as position size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::state::RiskState;
    use crate::domain::trading::portfolio::Portfolio;
    use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn create_test_candles(prices: &[f64]) -> Vec<Candle> {
        prices
            .iter()
            .enumerate()
            .map(|(i, &price)| {
                let dec_price = Decimal::from_f64_retain(price).unwrap();
                Candle {
                    symbol: "BTC/USD".to_string(),
                    open: dec_price,
                    high: dec_price,
                    low: dec_price,
                    close: dec_price,
                    volume: 1000.0,              // f64 type
                    timestamp: i as i64 * 60000, // 1 minute apart
                }
            })
            .collect()
    }

    fn create_test_proposal(price: Decimal) -> TradeProposal {
        TradeProposal {
            symbol: "BTC/USD".to_string(),
            side: OrderSide::Buy,
            price,
            quantity: dec!(0.1),
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_approve_normal_price() {
        let validator = PriceAnomalyValidator::new(PriceAnomalyConfig::default());

        // SMA of [50000, 50100, 50200, 50150, 50050] = 50100
        let candles = create_test_candles(&[50000.0, 50100.0, 50200.0, 50150.0, 50050.0]);

        // Price at 50100 = exactly at SMA, 0% deviation
        let proposal = create_test_proposal(dec!(50100));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(100000),
            None, // recent_candles
        );

        // Manually add candles to context (in real usage, this would be passed by RiskManager)
        let ctx_with_candles = ValidationContext {
            recent_candles: Some(&candles),
            ..ctx
        };

        let result = validator.validate(&ctx_with_candles).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_approve_small_deviation() {
        let validator = PriceAnomalyValidator::new(PriceAnomalyConfig::default());

        // SMA = 50000
        let candles = create_test_candles(&[50000.0, 50000.0, 50000.0, 50000.0, 50000.0]);

        // Price at 51500 = +3% deviation (under 5% threshold)
        let proposal = create_test_proposal(dec!(51500));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let mut ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(100000),
            None, // recent_candles
        );
        ctx.recent_candles = Some(&candles);

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_reject_high_deviation() {
        let validator = PriceAnomalyValidator::new(PriceAnomalyConfig::default());

        // SMA = 50000
        let candles = create_test_candles(&[50000.0, 50000.0, 50000.0, 50000.0, 50000.0]);

        // Price at 53000 = +6% deviation (exceeds 5% threshold)
        let proposal = create_test_proposal(dec!(53000));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let mut ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(100000),
            None, // recent_candles
        );
        ctx.recent_candles = Some(&candles);

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("6.00%"));
        assert!(result.rejection_reason().unwrap().contains("5.00%"));
        assert!(
            result
                .rejection_reason()
                .unwrap()
                .contains("Price anomaly detected")
        );
    }

    #[tokio::test]
    async fn test_reject_low_deviation() {
        let validator = PriceAnomalyValidator::new(PriceAnomalyConfig::default());

        // SMA = 50000
        let candles = create_test_candles(&[50000.0, 50000.0, 50000.0, 50000.0, 50000.0]);

        // Price at 46500 = -7% deviation (exceeds 5% threshold)
        let proposal = create_test_proposal(dec!(46500));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let mut ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(100000),
            None, // recent_candles
        );
        ctx.recent_candles = Some(&candles);

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("7.00%"));
    }

    #[tokio::test]
    async fn test_approve_insufficient_data() {
        let validator = PriceAnomalyValidator::new(PriceAnomalyConfig::default());

        // Only 3 candles (need 5 for SMA)
        let candles = create_test_candles(&[50000.0, 51000.0, 52000.0]);

        // Extreme price should still be approved due to insufficient data
        let proposal = create_test_proposal(dec!(100000));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let mut ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(100000),
            None, // recent_candles
        );
        ctx.recent_candles = Some(&candles);

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_approve_no_candles() {
        let validator = PriceAnomalyValidator::new(PriceAnomalyConfig::default());

        let proposal = create_test_proposal(dec!(50000));
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(
            &proposal,
            &portfolio,
            dec!(100000),
            &prices,
            &risk_state,
            None,
            None,
            None,
            Decimal::ZERO,
            dec!(100000),
            None, // recent_candles
        );
        // No candles provided

        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[test]
    fn test_sma_calculation() {
        let validator = PriceAnomalyValidator::new(PriceAnomalyConfig::default());
        let candles = create_test_candles(&[100.0, 200.0, 300.0, 400.0, 500.0]);

        let sma = validator.calculate_sma(&candles).unwrap();
        assert_eq!(sma, dec!(300)); // (100+200+300+400+500)/5 = 300
    }
}
