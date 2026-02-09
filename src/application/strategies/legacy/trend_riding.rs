use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::Decimal;

/// Trend Riding Strategy
///
/// Designed to capture and ride strong trends:
/// - Buy when golden cross occurs above trend SMA
/// - Hold position with buffer zone to avoid whipsaws
/// - Exit only when price drops significantly below trend
#[derive(Debug, Clone)]
pub struct TrendRidingStrategy {
    #[allow(dead_code)]
    fast_period: usize,
    #[allow(dead_code)]
    slow_period: usize,
    sma_threshold: Decimal,
    exit_buffer_pct: Decimal, // Buffer below trend SMA before exiting
}

impl TrendRidingStrategy {
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        sma_threshold: Decimal,
        exit_buffer_pct: Decimal,
    ) -> Self {
        Self {
            fast_period,
            slow_period,
            sma_threshold,
            exit_buffer_pct,
        }
    }
}

impl TradingStrategy for TrendRidingStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let fast = ctx.fast_sma;
        let slow = ctx.slow_sma;

        // Buy: Golden cross above trend SMA
        if fast > slow * (Decimal::ONE + self.sma_threshold) && ctx.current_price > ctx.trend_sma {
            return Some(Signal::buy(format!(
                "TrendRiding: Golden Cross above Trend (price={}, trend={})",
                ctx.current_price, ctx.trend_sma
            )));
        }

        // Sell: Price drops below trend SMA with buffer
        if ctx.has_position {
            let exit_threshold = ctx.trend_sma * (Decimal::ONE - self.exit_buffer_pct);
            if ctx.current_price < exit_threshold {
                return Some(Signal::sell(format!(
                    "TrendRiding: Price below trend buffer (price={}, threshold={})",
                    ctx.current_price, exit_threshold
                )));
            }
        }

        None
    }

    fn name(&self) -> &str {
        "TrendRiding"
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
        price: Decimal,
        trend_sma: Decimal,
        has_position: bool,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: price,
            price_f64: 0.0,
            fast_sma,
            slow_sma,
            trend_sma,
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
            candles: std::collections::VecDeque::new(),
            rsi_history: std::collections::VecDeque::new(),
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
    fn test_trend_riding_buy_above_trend() {
        let strategy = TrendRidingStrategy::new(20, 60, dec!(0.001), dec!(0.03));
        let ctx = create_test_context(dec!(105.0), dec!(100.0), dec!(110.0), dec!(100.0), false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("TrendRiding"));
    }

    #[test]
    fn test_trend_riding_no_buy_below_trend() {
        let strategy = TrendRidingStrategy::new(20, 60, dec!(0.001), dec!(0.03));
        let ctx = create_test_context(dec!(105.0), dec!(100.0), dec!(95.0), dec!(100.0), false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should not buy below trend SMA");
    }

    #[test]
    fn test_trend_riding_exit_below_buffer() {
        let strategy = TrendRidingStrategy::new(20, 60, dec!(0.001), dec!(0.03));
        // Trend SMA = 100, buffer = 3%, exit threshold = 97
        let ctx = create_test_context(dec!(98.0), dec!(100.0), dec!(96.0), dec!(100.0), true);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert!(sig.reason.contains("below trend buffer"));
    }

    #[test]
    fn test_trend_riding_hold_within_buffer() {
        let strategy = TrendRidingStrategy::new(20, 60, dec!(0.001), dec!(0.03));
        // Price at 98, trend at 100, buffer threshold at 97
        // Should hold (not exit yet)
        let ctx = create_test_context(dec!(98.0), dec!(100.0), dec!(98.0), dec!(100.0), true);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should hold within buffer zone");
    }
}
