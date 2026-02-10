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
    pub rsi_oversold_zone: Decimal, // RSI threshold for bullish divergence (default: 40.0)
    pub rsi_overbought_zone: Decimal, // RSI threshold for bearish divergence (default: 60.0)
}

impl MomentumDivergenceStrategy {
    pub fn new(divergence_lookback: usize, min_divergence_pct: Decimal) -> Self {
        Self {
            divergence_lookback,
            min_divergence_pct,
            rsi_oversold_zone: dec!(40.0),
            rsi_overbought_zone: dec!(60.0),
        }
    }

    /// Find local extremes (highs and lows) in price and RSI
    /// Returns: (price_low1, price_low2, rsi_at_low1, rsi_at_low2) for bullish
    /// or (price_high1, price_high2, rsi_at_high1, rsi_at_high2) for bearish
    /// Find local extremes (highs and lows) in price and RSI
    /// Returns: (price_low1, price_low2, rsi_at_low1, rsi_at_low2) for bullish
    /// or (price_high1, price_high2, rsi_at_high1, rsi_at_high2) for bearish
    pub(crate) fn find_divergence(&self, ctx: &AnalysisContext) -> Option<DivergenceType> {
        if ctx.candles.len() < self.divergence_lookback {
            return None;
        }

        // Ensure RSI history is sufficiently aligned with candles
        if ctx.rsi_history.len() < self.divergence_lookback / 2 {
            tracing::debug!("Insufficient RSI history for divergence detection");
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
        let mut second_low_idx = 0;

        let mut first_high = Decimal::MIN;
        let mut first_high_idx = 0;
        let mut second_high = Decimal::MIN;
        let mut second_high_idx = 0;

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

        // Analyze second half for second extreme (recent extreme)
        for (i, candle) in ctx.candles.iter().enumerate().skip(mid_point) {
            let low = candle.low;
            let high = candle.high;

            if low < second_low {
                second_low = low;
                second_low_idx = i;
            }
            if high > second_high {
                second_high = high;
                second_high_idx = i;
            }
        }

        // Helper to safely get RSI
        let get_rsi_at = |idx: usize| -> Option<Decimal> {
            if idx >= rsi_offset {
                ctx.rsi_history.get(idx - rsi_offset).copied()
            } else {
                None
            }
        };

        // Get RSI at specific points
        let rsi_at_first_low = get_rsi_at(first_low_idx)?;
        let rsi_at_second_low = get_rsi_at(second_low_idx)?;

        let rsi_at_first_high = get_rsi_at(first_high_idx)?;
        let rsi_at_second_high = get_rsi_at(second_high_idx)?;

        // Check for bullish divergence: lower low in price, higher low in RSI
        // Price low 2 < Price low 1
        // RSI at low 2 > RSI at low 1
        let price_lower_low = second_low < first_low * (Decimal::ONE - self.min_divergence_pct);

        if !ctx.has_position
            && price_lower_low
            && rsi_at_second_low < self.rsi_oversold_zone
            && rsi_at_second_low > rsi_at_first_low
        {
            return Some(DivergenceType::Bullish {
                price_low1: first_low,
                price_low2: second_low,
                rsi_now: rsi_at_second_low, // We report the RSI at the divergence point
            });
        }

        // Check for bearish divergence: higher high in price, lower high in RSI
        let price_higher_high = second_high > first_high * (Decimal::ONE + self.min_divergence_pct);

        if ctx.has_position
            && price_higher_high
            && rsi_at_second_high > self.rsi_overbought_zone
            && rsi_at_second_high < rsi_at_first_high
        {
            return Some(DivergenceType::Bearish {
                price_high1: first_high,
                price_high2: second_high,
                rsi_now: rsi_at_second_high,
            });
        }

        None
    }
}

#[derive(Debug)]
pub(crate) enum DivergenceType {
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
            rsi_oversold_zone: dec!(40.0),
            rsi_overbought_zone: dec!(60.0),
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
            position: None,
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
        // Price low 1 at index 6 or 7. Price low 2 at index 14.
        // RSI must be rising between those indices.
        // RSI index aligns with candles.
        let mut rsi_history = VecDeque::new();
        for i in 0..15 {
            rsi_history.push_back(dec!(20.0) + Decimal::from(i));
        }

        let ctx = create_context(80.0, 34.0, candles, rsi_history, false);
        // The strategy should find divergence
        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(sig.reason.contains("Momentum"));
        assert!(sig.reason.contains("Bullish"));
    }

    #[test]
    fn test_bearish_divergence_detection() {
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
        // Price High 1 ~ idx 7. Price High 2 ~ idx 14.
        // RSI should be falling.
        let mut rsi_history = VecDeque::new();
        for i in 0..15 {
            rsi_history.push_back(dec!(80.0) - Decimal::from(i));
        }

        let ctx = create_context(120.0, 66.0, candles, rsi_history, true);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(sig.reason.contains("Momentum"));
        assert!(sig.reason.contains("Bearish"));
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
