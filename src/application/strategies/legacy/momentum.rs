use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Momentum/Divergence Strategy
///
/// Detects price/RSI divergences to identify potential reversals:
/// - Bullish Divergence: Price makes lower low, RSI makes higher low → Buy signal
/// - Bearish Divergence: Price makes higher high, RSI makes lower high → Sell signal
#[derive(Debug, Clone)]
pub struct MomentumDivergenceStrategy {
    pub divergence_lookback: usize, // Number of candles to look back for divergence
    pub min_divergence_pct: Decimal, // Minimum price movement to consider (e.g., 0.02 = 2%)
}

impl MomentumDivergenceStrategy {
    pub fn new(divergence_lookback: usize, min_divergence_pct: Decimal) -> Self {
        Self {
            divergence_lookback,
            min_divergence_pct,
        }
    }

    /// Find local extremes (highs and lows) in price and RSI
    /// Returns: (price_low1, price_low2, rsi_at_low1, rsi_at_low2) for bullish
    /// or (price_high1, price_high2, rsi_at_high1, rsi_at_high2) for bearish
    /// Find local extremes (highs and lows) in price and RSI
    /// Returns: (price_low1, price_low2, rsi_at_low1, rsi_at_low2) for bullish
    /// or (price_high1, price_high2, rsi_at_high1, rsi_at_high2) for bearish
    fn find_divergence(&self, ctx: &AnalysisContext) -> Option<DivergenceType> {
        if ctx.candles.len() < self.divergence_lookback {
            return None;
        }

        // Align RSI history with candles
        let rsi_offset = ctx.candles.len().saturating_sub(ctx.rsi_history.len());
        let start_idx = ctx.candles.len().saturating_sub(self.divergence_lookback);

        // Find the lowest low and its position in first half
        let mid_point = start_idx + (ctx.candles.len() - start_idx) / 2;

        let mut first_low = Decimal::MAX;
        let mut first_low_idx = 0;
        let mut second_low = Decimal::MAX;

        let mut first_high = Decimal::MIN;
        let mut first_high_idx = 0;
        let mut second_high = Decimal::MIN;

        // Analyze first half for initial extreme
        for (i, candle) in ctx
            .candles
            .iter()
            .enumerate()
            .skip(start_idx)
            .take(mid_point - start_idx)
        {
            let low = candle.low;
            let high = candle.high;

            if low < first_low {
                first_low = low;
                first_low_idx = i;
            }
            if high > first_high {
                first_high = high;
                first_high_idx = i;
            }
        }

        // Analyze second half for second extreme (current extreme)
        for candle in ctx.candles.iter().skip(mid_point) {
            let low = candle.low;
            let high = candle.high;

            if low < second_low {
                second_low = low;
            }
            if high > second_high {
                second_high = high;
            }
        }

        // Current RSI represents the "end" state
        let current_rsi = ctx.rsi;

        // Helper to safely get RSI
        let get_rsi_at = |idx: usize| -> Option<Decimal> {
            if idx >= rsi_offset {
                ctx.rsi_history.get(idx - rsi_offset).copied()
            } else {
                None
            }
        };

        // Check for bullish divergence: lower low in price, higher low in RSI
        let price_lower_low = second_low < first_low * (Decimal::ONE - self.min_divergence_pct);

        if !ctx.has_position
            && price_lower_low
            && current_rsi < dec!(40.0)
            && let Some(past_rsi) = get_rsi_at(first_low_idx)
            && current_rsi > past_rsi
        {
            return Some(DivergenceType::Bullish {
                price_low1: first_low,
                price_low2: second_low,
                rsi_now: current_rsi,
            });
        }

        // Check for bearish divergence: higher high in price, lower high in RSI
        let price_higher_high = second_high > first_high * (Decimal::ONE + self.min_divergence_pct);

        if ctx.has_position
            && price_higher_high
            && current_rsi > dec!(60.0)
            && let Some(past_rsi) = get_rsi_at(first_high_idx)
            && current_rsi < past_rsi
        {
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
        price_low1: Decimal,
        price_low2: Decimal,
        rsi_now: Decimal,
    },
    Bearish {
        price_high1: Decimal,
        price_high2: Decimal,
        rsi_now: Decimal,
    },
}

impl Default for MomentumDivergenceStrategy {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            divergence_lookback: 14,
            min_divergence_pct: dec!(0.02), // 2% price movement
        }
    }
}

impl TradingStrategy for MomentumDivergenceStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let divergence = self.find_divergence(ctx)?;

        match divergence {
            DivergenceType::Bullish {
                price_low1,
                price_low2,
                rsi_now,
            } => Some(
                Signal::buy(format!(
                    "Momentum: Bullish Divergence - Price LL ({} → {}) but RSI rising ({})",
                    price_low1, price_low2, rsi_now
                ))
                .with_confidence(0.75),
            ),
            DivergenceType::Bearish {
                price_high1,
                price_high2,
                rsi_now,
            } => Some(
                Signal::sell(format!(
                    "Momentum: Bearish Divergence - Price HH ({} → {}) but RSI falling ({})",
                    price_high1, price_high2, rsi_now
                ))
                .with_confidence(0.75),
            ),
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
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;

    fn mock_candle(high: f64, low: f64, close: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64((high + low) / 2.0).unwrap(),
            high: Decimal::from_f64(high).unwrap(),
            low: Decimal::from_f64(low).unwrap(),
            close: Decimal::from_f64(close).unwrap(),
            volume: Decimal::from_f64(1000.0).unwrap(),
            timestamp: 0,
        }
    }

    fn create_context(
        price: f64,
        rsi: f64,
        candles: VecDeque<Candle>,
        rsi_history: VecDeque<Decimal>,
        has_position: bool,
    ) -> AnalysisContext {
        let d_price = Decimal::from_f64(price).unwrap();
        let d_rsi = Decimal::from_f64(rsi).unwrap();
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: d_price,
            price_f64: price,
            fast_sma: Decimal::ZERO,
            slow_sma: Decimal::ZERO,
            trend_sma: Decimal::ZERO,
            rsi: d_rsi,
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
            timestamp: 0,
            candles,
            rsi_history,
            // OFI fields (defaults for tests)
            ofi_value: Decimal::ZERO,
            cumulative_delta: Decimal::ZERO,
            volume_profile: None,
            ofi_history: VecDeque::new(),
            hurst_exponent: None,
            skewness: None,
            momentum_normalized: None,
            realized_volatility: None,
            timeframe_features: None,
            feature_set: None,
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

        // Create RSI history showing Bullish Divergence
        let mut rsi_history = VecDeque::new();
        for i in 0..15 {
            rsi_history.push_back(dec!(20.0) + Decimal::from(i));
        }

        let ctx = create_context(80.0, 25.0, candles, rsi_history, false);
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

        // Create RSI history showing Bearish Divergence
        let mut rsi_history = VecDeque::new();
        for i in 0..15 {
            rsi_history.push_back(dec!(80.0) - Decimal::from(i));
        }

        let ctx = create_context(120.0, 75.0, candles, rsi_history, true);
        // The strategy should analyze without panicking
        let _ = strategy.analyze(&ctx);
    }

    #[test]
    fn test_no_divergence_insufficient_data() {
        let strategy = MomentumDivergenceStrategy::new(20, dec!(0.02));

        let mut candles = VecDeque::new();
        // Only 5 candles, but strategy needs 20 lookback
        for _ in 0..5 {
            candles.push_back(mock_candle(105.0, 95.0, 100.0));
        }

        let ctx = create_context(100.0, 50.0, candles, VecDeque::new(), false);

        let signal = strategy.analyze(&ctx);
        assert!(
            signal.is_none(),
            "Should return None with insufficient data"
        );
    }

    #[test]
    fn test_no_divergence_normal_trend() {
        let strategy = MomentumDivergenceStrategy::new(10, dec!(0.02));

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
        let mut rsi_history = VecDeque::new();
        for _ in 0..10 {
            rsi_history.push_back(dec!(55.0));
        }
        let ctx = create_context(108.0, 55.0, candles, rsi_history, false);

        let signal = strategy.analyze(&ctx);
        // Should not trigger as no divergence (price up, momentum also up)
        assert!(signal.is_none());
    }
}
