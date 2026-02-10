use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::Decimal;

/// Dual Simple Moving Average (SMA) crossover strategy
///
/// Generates buy signal when fast SMA crosses above slow SMA
/// Generates sell signal when fast SMA crosses below slow SMA
#[derive(Debug, Clone)]
pub struct DualSMAStrategy {
    pub fast_period: usize,
    pub slow_period: usize,
    pub threshold: Decimal,
}

impl DualSMAStrategy {
    pub fn new(fast_period: usize, slow_period: usize, threshold: Decimal) -> Self {
        Self {
            fast_period,
            slow_period,
            threshold,
        }
    }
}

impl TradingStrategy for DualSMAStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let fast = ctx.fast_sma;
        let slow = ctx.slow_sma;

        // Buy: Golden cross (fast SMA crosses above slow SMA)
        // Guard: only emit buy if no position open (avoid spam in sustained uptrend)
        if !ctx.has_position && fast > slow * (Decimal::ONE + self.threshold) {
            return Some(Signal::buy(format!(
                "Golden Cross (Fast={} > Slow={})",
                fast, slow
            )));
        }

        // Sell: Death cross or trend reversal (exit on either condition)
        if ctx.has_position {
            let death_cross = fast < slow * (Decimal::ONE - self.threshold);
            let trend_break = ctx.current_price < ctx.trend_sma;

            if death_cross || trend_break {
                let reason = if death_cross {
                    "Death Cross"
                } else {
                    "Trend Break"
                };
                return Some(Signal::sell(format!(
                    "{} (Fast={}, Slow={}, Price={}, Trend={})",
                    reason, fast, slow, ctx.current_price, ctx.trend_sma
                )));
            }
        }

        None
    }

    fn name(&self) -> &str {
        "DualSMA"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::OrderSide;
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;

    fn create_test_context(
        fast_sma: Decimal,
        slow_sma: Decimal,
        has_position: bool,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
            price_f64: 100.0,
            fast_sma,
            slow_sma,
            trend_sma: dec!(99.0), // Below price to allow buy signals
            rsi: dec!(50.0),
            macd_value: Decimal::ZERO,
            macd_signal: Decimal::ZERO,
            macd_histogram: Decimal::ZERO,
            last_macd_histogram: None,
            atr: Decimal::ONE,
            bb_lower: Decimal::ZERO,
            bb_middle: Decimal::ZERO,
            bb_upper: Decimal::ZERO,
            adx: Decimal::ZERO,
            has_position,
            position: None,
            timestamp: 0,
            timeframe_features: None,
            candles: VecDeque::new(),
            rsi_history: VecDeque::new(),
            // OFI fields (defaults for tests)
            ofi_value: Decimal::ZERO,
            cumulative_delta: Decimal::ZERO,
            volume_profile: None,
            ofi_history: VecDeque::new(),
            hurst_exponent: None,
            skewness: None,
            momentum_normalized: None,
            realized_volatility: None,
            feature_set: None,
        }
    }

    #[test]
    fn test_golden_cross_buy_signal() {
        let strategy = DualSMAStrategy::new(20, 60, dec!(0.001));
        let ctx = create_test_context(dec!(102.0), dec!(100.0), false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Golden Cross"));
    }

    #[test]
    fn test_death_cross_sell_signal() {
        let strategy = DualSMAStrategy::new(20, 60, dec!(0.001));
        let mut ctx = create_test_context(dec!(98.0), dec!(100.0), true);
        ctx.has_position = true; // Must have position to sell

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert!(sig.reason.contains("Death Cross"));
    }

    #[test]
    fn test_no_signal_when_smas_close() {
        let strategy = DualSMAStrategy::new(20, 60, dec!(0.001));
        let ctx = create_test_context(dec!(100.05), dec!(100.0), false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none());
    }

    #[test]
    fn test_no_sell_without_position() {
        let strategy = DualSMAStrategy::new(20, 60, dec!(0.001));
        let ctx = create_test_context(dec!(98.0), dec!(100.0), false); // has_position = false

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should not sell without position");
    }
}
