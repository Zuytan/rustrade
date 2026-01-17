use super::traits::{AnalysisContext, Signal, TradingStrategy};

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
    sma_threshold: f64,
    exit_buffer_pct: f64, // Buffer below trend SMA before exiting
}

impl TrendRidingStrategy {
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        sma_threshold: f64,
        exit_buffer_pct: f64,
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
        if fast > slow * (1.0 + self.sma_threshold) && ctx.price_f64 > ctx.trend_sma {
            return Some(Signal::buy(format!(
                "TrendRiding: Golden Cross above Trend (price={:.2}, trend={:.2})",
                ctx.price_f64, ctx.trend_sma
            )));
        }

        // Sell: Price drops below trend SMA with buffer
        if ctx.has_position {
            let exit_threshold = ctx.trend_sma * (1.0 - self.exit_buffer_pct);
            if ctx.price_f64 < exit_threshold {
                return Some(Signal::sell(format!(
                    "TrendRiding: Price below trend buffer (price={:.2}, threshold={:.2})",
                    ctx.price_f64, exit_threshold
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
        fast_sma: f64,
        slow_sma: f64,
        price: f64,
        trend_sma: f64,
        has_position: bool,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
            price_f64: price,
            fast_sma,
            slow_sma,
            trend_sma,
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
            timeframe_features: None,
            candles: std::collections::VecDeque::new(),
            rsi_history: std::collections::VecDeque::new(),
            // OFI fields (defaults for tests)
            ofi_value: 0.0,
            cumulative_delta: 0.0,
            volume_profile: None,
            ofi_history: VecDeque::new(),
        }
    }

    #[test]
    fn test_trend_riding_buy_above_trend() {
        let strategy = TrendRidingStrategy::new(20, 60, 0.001, 0.03);
        let ctx = create_test_context(105.0, 100.0, 110.0, 100.0, false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("TrendRiding"));
    }

    #[test]
    fn test_trend_riding_no_buy_below_trend() {
        let strategy = TrendRidingStrategy::new(20, 60, 0.001, 0.03);
        let ctx = create_test_context(105.0, 100.0, 95.0, 100.0, false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should not buy below trend SMA");
    }

    #[test]
    fn test_trend_riding_exit_below_buffer() {
        let strategy = TrendRidingStrategy::new(20, 60, 0.001, 0.03);
        // Trend SMA = 100, buffer = 3%, exit threshold = 97
        let ctx = create_test_context(98.0, 100.0, 96.0, 100.0, true);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert!(sig.reason.contains("below trend buffer"));
    }

    #[test]
    fn test_trend_riding_hold_within_buffer() {
        let strategy = TrendRidingStrategy::new(20, 60, 0.001, 0.03);
        // Price at 98, trend at 100, buffer threshold at 97
        // Should hold (not exit yet)
        let ctx = create_test_context(98.0, 100.0, 98.0, 100.0, true);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should hold within buffer zone");
    }
}
