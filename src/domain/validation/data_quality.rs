use crate::domain::trading::types::{Candle, MarketEvent};
use rust_decimal::Decimal;
use tracing::warn;

/// Centralized validator for market data integrity.
///
/// Rejects data that is physically impossible or highly suspect (e.g. flash crashes).
pub struct StrictEventValidator;

impl StrictEventValidator {
    /// Validates a MarketEvent. Returns true if valid, false otherwise.
    pub fn validate_event(event: &MarketEvent) -> bool {
        match event {
            MarketEvent::Quote {
                symbol,
                price,
                quantity,
                ..
            } => {
                if *price <= Decimal::ZERO {
                    warn!(
                        "Validation FAILED: Symbol {} has non-positive price: {}",
                        symbol, price
                    );
                    return false;
                }
                if *quantity < Decimal::ZERO {
                    warn!(
                        "Validation FAILED: Symbol {} has negative quantity: {}",
                        symbol, quantity
                    );
                    return false;
                }
                true
            }
            MarketEvent::Candle(candle) => Self::validate_candle(candle),
            MarketEvent::SymbolSubscription { .. } => true, // Meta-events are always valid
        }
    }

    /// Validates a Candle.
    pub fn validate_candle(candle: &Candle) -> bool {
        if candle.open <= Decimal::ZERO
            || candle.high <= Decimal::ZERO
            || candle.low <= Decimal::ZERO
            || candle.close <= Decimal::ZERO
        {
            warn!(
                "Validation FAILED: Candle for {} has non-positive price component(s)",
                candle.symbol
            );
            return false;
        }

        if candle.low > candle.high {
            warn!(
                "Validation FAILED: Candle for {} has low {} > high {}",
                candle.symbol, candle.low, candle.high
            );
            return false;
        }

        if candle.volume < Decimal::ZERO {
            warn!(
                "Validation FAILED: Candle for {} has negative volume: {}",
                candle.symbol, candle.volume
            );
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_validate_quote_positive() {
        let event = MarketEvent::Quote {
            symbol: "BTC/USDT".to_string(),
            price: dec!(50000.0),
            quantity: dec!(0.1),
            timestamp: 123,
        };
        assert!(StrictEventValidator::validate_event(&event));
    }

    #[test]
    fn test_validate_quote_negative_price() {
        let event = MarketEvent::Quote {
            symbol: "BTC/USDT".to_string(),
            price: dec!(-1.0),
            quantity: dec!(0.1),
            timestamp: 123,
        };
        assert!(!StrictEventValidator::validate_event(&event));
    }

    #[test]
    fn test_validate_candle_invalid_low_high() {
        let candle = Candle {
            symbol: "ETH/USDT".to_string(),
            open: dec!(2000.0),
            high: dec!(2000.0),
            low: dec!(2001.0), // Low > High
            close: dec!(2000.0),
            volume: dec!(100.0),
            timestamp: 123,
        };
        assert!(!StrictEventValidator::validate_candle(&candle));
    }
}
