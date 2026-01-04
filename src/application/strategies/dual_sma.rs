use super::traits::{AnalysisContext, Signal, TradingStrategy};

/// Dual Simple Moving Average (SMA) crossover strategy
///
/// Generates buy signal when fast SMA crosses above slow SMA
/// Generates sell signal when fast SMA crosses below slow SMA
#[derive(Debug, Clone)]
pub struct DualSMAStrategy {
    pub fast_period: usize,
    pub slow_period: usize,
    pub threshold: f64,
}

impl DualSMAStrategy {
    pub fn new(fast_period: usize, slow_period: usize, threshold: f64) -> Self {
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

        // Golden Cross: Fast SMA crosses above Slow SMA
        if fast > slow * (1.0 + self.threshold) {
            tracing::debug!(
                "DualSMA [{}]: Golden Cross detected (fast={:.2}, slow={:.2}, threshold={:.4})",
                ctx.symbol,
                fast,
                slow,
                self.threshold
            );
            return Some(Signal::buy(format!(
                "Golden Cross (Fast={:.2} > Slow={:.2})",
                fast, slow
            )));
        }

        // Death Cross: Fast SMA crosses below Slow SMA
        if fast < slow * (1.0 - self.threshold) && ctx.has_position {
            tracing::debug!(
                "DualSMA [{}]: Death Cross detected (fast={:.2}, slow={:.2}, has_pos=true)",
                ctx.symbol,
                fast,
                slow
            );
            return Some(Signal::sell(format!(
                "Death Cross (Fast={:.2} < Slow={:.2})",
                fast, slow
            )));
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

    fn create_test_context(fast_sma: f64, slow_sma: f64, has_position: bool) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
            price_f64: 100.0,
            fast_sma,
            slow_sma,
            trend_sma: 100.0,
            rsi: 50.0,
            macd_value: 0.0,
            macd_signal: 0.0,
            macd_histogram: 0.0,
            last_macd_histogram: None,
            atr: 1.0,
            bb_lower: 0.0,
            bb_middle: 0.0,
            bb_upper: 0.0,
            adx: 0.0,
            has_position,
            timestamp: 0,
        }
    }

    #[test]
    fn test_golden_cross_buy_signal() {
        let strategy = DualSMAStrategy::new(20, 60, 0.001);
        let ctx = create_test_context(102.0, 100.0, false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Golden Cross"));
    }

    #[test]
    fn test_death_cross_sell_signal() {
        let strategy = DualSMAStrategy::new(20, 60, 0.001);
        let mut ctx = create_test_context(98.0, 100.0, true);
        ctx.has_position = true; // Must have position to sell

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert!(sig.reason.contains("Death Cross"));
    }

    #[test]
    fn test_no_signal_when_smas_close() {
        let strategy = DualSMAStrategy::new(20, 60, 0.001);
        let ctx = create_test_context(100.05, 100.0, false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none());
    }

    #[test]
    fn test_no_sell_without_position() {
        let strategy = DualSMAStrategy::new(20, 60, 0.001);
        let ctx = create_test_context(98.0, 100.0, false); // has_position = false

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should not sell without position");
    }
}
