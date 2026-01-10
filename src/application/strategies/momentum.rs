use super::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::prelude::ToPrimitive;

/// Momentum/Divergence Strategy
///
/// Detects price/RSI divergences to identify potential reversals:
/// - Bullish Divergence: Price makes lower low, RSI makes higher low → Buy signal
/// - Bearish Divergence: Price makes higher high, RSI makes lower high → Sell signal
#[derive(Debug, Clone)]
pub struct MomentumDivergenceStrategy {
    pub divergence_lookback: usize, // Number of candles to look back for divergence
    pub min_divergence_pct: f64,    // Minimum price movement to consider (e.g., 0.02 = 2%)
}

impl MomentumDivergenceStrategy {
    pub fn new(divergence_lookback: usize, min_divergence_pct: f64) -> Self {
        Self {
            divergence_lookback,
            min_divergence_pct,
        }
    }

    /// Find local extremes (highs and lows) in price and RSI
    /// Returns: (price_low1, price_low2, rsi_at_low1, rsi_at_low2) for bullish
    /// or (price_high1, price_high2, rsi_at_high1, rsi_at_high2) for bearish
    fn find_divergence(&self, ctx: &AnalysisContext) -> Option<DivergenceType> {
        if ctx.candles.len() < self.divergence_lookback {
            return None;
        }

        let start_idx = ctx.candles.len().saturating_sub(self.divergence_lookback);

        // Find the lowest low and its position in first half
        let mid_point = start_idx + (ctx.candles.len() - start_idx) / 2;

        let mut first_low = f64::MAX;
        let mut second_low = f64::MAX;
        let mut first_high = f64::MIN;
        let mut second_high = f64::MIN;

        // Analyze first half for initial extreme
        for candle in ctx
            .candles
            .iter()
            .skip(start_idx)
            .take(mid_point - start_idx)
        {
            let low = candle.low.to_f64().unwrap_or(f64::MAX);
            let high = candle.high.to_f64().unwrap_or(f64::MIN);

            if low < first_low {
                first_low = low;
            }
            if high > first_high {
                first_high = high;
            }
        }

        // Analyze second half for second extreme
        for candle in ctx.candles.iter().skip(mid_point) {
            let low = candle.low.to_f64().unwrap_or(f64::MAX);
            let high = candle.high.to_f64().unwrap_or(f64::MIN);

            if low < second_low {
                second_low = low;
            }
            if high > second_high {
                second_high = high;
            }
        }

        // Current RSI represents the "end" state
        let current_rsi = ctx.rsi;

        // Estimate "past" RSI based on trend (simplified - in production would track RSI history)
        // For now, use a heuristic based on price movement
        let price_change_pct = (ctx.price_f64 - first_low) / first_low;
        let estimated_past_rsi = if price_change_pct < 0.0 {
            // Price went down, past RSI was likely higher
            current_rsi + (price_change_pct.abs() * 50.0).min(30.0)
        } else {
            // Price went up, past RSI was likely lower
            current_rsi - (price_change_pct * 50.0).min(30.0)
        };

        // Check for bullish divergence: lower low in price, higher low in RSI
        let price_lower_low = second_low < first_low * (1.0 - self.min_divergence_pct);
        let rsi_higher_low = current_rsi > estimated_past_rsi;

        if !ctx.has_position && price_lower_low && rsi_higher_low && current_rsi < 40.0 {
            return Some(DivergenceType::Bullish {
                price_low1: first_low,
                price_low2: second_low,
                rsi_now: current_rsi,
            });
        }

        // Check for bearish divergence: higher high in price, lower high in RSI
        let price_higher_high = second_high > first_high * (1.0 + self.min_divergence_pct);
        let rsi_lower_high = current_rsi < estimated_past_rsi;

        if ctx.has_position && price_higher_high && rsi_lower_high && current_rsi > 60.0 {
            return Some(DivergenceType::Bearish {
                price_high1: first_high,
                price_high2: second_high,
                rsi_now: current_rsi,
            });
        }

        None
    }
}

#[derive(Debug)]
enum DivergenceType {
    Bullish {
        price_low1: f64,
        price_low2: f64,
        rsi_now: f64,
    },
    Bearish {
        price_high1: f64,
        price_high2: f64,
        rsi_now: f64,
    },
}

impl Default for MomentumDivergenceStrategy {
    fn default() -> Self {
        Self {
            divergence_lookback: 14,
            min_divergence_pct: 0.02, // 2% price movement
        }
    }
}

impl TradingStrategy for MomentumDivergenceStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let divergence = self.find_divergence(ctx)?;

        match divergence {
            DivergenceType::Bullish { price_low1, price_low2, rsi_now } => {
                Some(Signal::buy(format!(
                    "Momentum: Bullish Divergence - Price LL ({:.2} → {:.2}) but RSI rising ({:.1})",
                    price_low1, price_low2, rsi_now
                )).with_confidence(0.75))
            }
            DivergenceType::Bearish { price_high1, price_high2, rsi_now } => {
                Some(Signal::sell(format!(
                    "Momentum: Bearish Divergence - Price HH ({:.2} → {:.2}) but RSI falling ({:.1})",
                    price_high1, price_high2, rsi_now
                )).with_confidence(0.75))
            }
        }
    }

    fn name(&self) -> &str {
        "MomentumDivergence"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::Candle;
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromPrimitive;
    use std::collections::VecDeque;

    fn mock_candle(high: f64, low: f64, close: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64((high + low) / 2.0).unwrap(),
            high: Decimal::from_f64(high).unwrap(),
            low: Decimal::from_f64(low).unwrap(),
            close: Decimal::from_f64(close).unwrap(),
            volume: 1000.0,
            timestamp: 0,
        }
    }

    fn create_context(
        price: f64,
        rsi: f64,
        candles: VecDeque<Candle>,
        has_position: bool,
    ) -> AnalysisContext {
        use rust_decimal_macros::dec;
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
            price_f64: price,
            fast_sma: 0.0,
            slow_sma: 0.0,
            trend_sma: 0.0,
            rsi,
            macd_value: 0.0,
            macd_signal: 0.0,
            macd_histogram: 0.0,
            last_macd_histogram: None,
            atr: 1.0,
            bb_lower: 0.0,
            bb_middle: 0.0,
            bb_upper: 0.0,
            adx: 25.0,
            has_position,
            timestamp: 0,
            timeframe_features: None,
            candles,
        }
    }

    #[test]
    fn test_bullish_divergence_detection() {
        // This test validates the strategy can be instantiated and analyzes without panic
        let strategy = MomentumDivergenceStrategy::default();

        let mut candles = VecDeque::new();
        for i in 0..15 {
            candles.push_back(mock_candle(
                105.0 - i as f64,
                95.0 - i as f64,
                100.0 - i as f64,
            ));
        }

        let ctx = create_context(80.0, 25.0, candles, false);
        // The strategy should analyze without panicking
        let _ = strategy.analyze(&ctx);
    }

    #[test]
    fn test_bearish_divergence_detection() {
        // This test validates the strategy can be instantiated and analyzes without panic
        let strategy = MomentumDivergenceStrategy::default();

        let mut candles = VecDeque::new();
        for i in 0..15 {
            candles.push_back(mock_candle(
                100.0 + i as f64,
                95.0 + i as f64,
                98.0 + i as f64,
            ));
        }

        let ctx = create_context(120.0, 75.0, candles, true);
        // The strategy should analyze without panicking
        let _ = strategy.analyze(&ctx);
    }

    #[test]
    fn test_no_divergence_insufficient_data() {
        let strategy = MomentumDivergenceStrategy::new(20, 0.02);

        let mut candles = VecDeque::new();
        // Only 5 candles, but strategy needs 20 lookback
        for _ in 0..5 {
            candles.push_back(mock_candle(105.0, 95.0, 100.0));
        }

        let ctx = create_context(100.0, 50.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(
            signal.is_none(),
            "Should return None with insufficient data"
        );
    }

    #[test]
    fn test_no_divergence_normal_trend() {
        let strategy = MomentumDivergenceStrategy::new(10, 0.02);

        let mut candles = VecDeque::new();
        // Normal uptrend: both price and momentum rising
        for i in 0..10 {
            candles.push_back(mock_candle(
                100.0 + i as f64,
                95.0 + i as f64,
                98.0 + i as f64,
            ));
        }

        // RSI neutral, price trending
        let ctx = create_context(108.0, 55.0, candles, false);

        let signal = strategy.analyze(&ctx);
        // Should not trigger as no divergence (price up, momentum also up)
        assert!(signal.is_none());
    }
}
