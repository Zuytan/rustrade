use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::Decimal;

/// Breakout Strategy
///
/// Detects breakouts from consolidation patterns by monitoring:
/// - Price breaking above recent highs (bullish breakout)
/// - Price breaking below recent lows (bearish breakout)
/// - Optional volume confirmation for stronger signals
#[derive(Debug, Clone)]
pub struct BreakoutStrategy {
    pub lookback_period: usize,          // Period to calculate high/low
    pub breakout_threshold_pct: Decimal, // % above/below high/low to confirm breakout
    pub volume_multiplier: Decimal,      // Required volume vs average (e.g., 1.5 = 50% above avg)
}

impl BreakoutStrategy {
    pub fn new(
        lookback_period: usize,
        breakout_threshold_pct: Decimal,
        volume_multiplier: Decimal,
    ) -> Self {
        Self {
            lookback_period,
            breakout_threshold_pct,
            volume_multiplier,
        }
    }

    /// Calculate recent high and low from candle history
    /// Excludes the most recent (current) candle to prevent looking ahead/comparing to self
    fn calculate_range(&self, ctx: &AnalysisContext) -> Option<(Decimal, Decimal, Decimal)> {
        // Need at least lookback + 1 candle (history + current)
        if ctx.candles.len() < self.lookback_period + 1 {
            return None;
        }

        // We want to look at the 'lookback_period' candles BEFORE the current one
        // ctx.candles.len() - 1 is the index of the last item (current)
        // So we take the range [len - 1 - lookback, len - 1)
        let end_idx = ctx.candles.len().saturating_sub(1);
        let start_idx = end_idx.saturating_sub(self.lookback_period);

        let mut highest_high = Decimal::MIN;
        let mut lowest_low = Decimal::MAX;
        let mut total_volume = Decimal::ZERO;
        let mut count = 0;

        // Iterating up to end_idx (exclusive) effectively excludes the current candle
        for candle in ctx
            .candles
            .iter()
            .skip(start_idx)
            .take(self.lookback_period)
        {
            let high = candle.high;
            let low = candle.low;

            if high > highest_high {
                highest_high = high;
            }
            if low < lowest_low {
                lowest_low = low;
            }
            total_volume += candle.volume;
            count += 1;
        }

        let avg_volume = if count > 0 {
            total_volume / Decimal::from(count)
        } else {
            Decimal::ZERO
        };

        Some((highest_high, lowest_low, avg_volume))
    }
}

impl Default for BreakoutStrategy {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            lookback_period: 10,                 // Reduced from 20 for faster detection
            breakout_threshold_pct: dec!(0.002), // 0.2% above high (reduced from 0.5%)
            volume_multiplier: dec!(1.1),        // 10% above average (reduced from 30%)
        }
    }
}

impl TradingStrategy for BreakoutStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if let Some((high, low, avg_vol)) = self.calculate_range(ctx) {
            let current_vol = ctx.candles.back()?.volume;
            let current_price = ctx.current_price;

            let vol_ok = current_vol >= avg_vol * self.volume_multiplier;

            // Breakout Long: Price breaks above recent high
            if !ctx.has_position
                && current_price > high * (Decimal::ONE + self.breakout_threshold_pct)
                && vol_ok
            {
                return Some(Signal::buy(format!(
                    "Bullish Breakout (Price={} > High={}, Vol={} > Avg={})",
                    current_price, high, current_vol, avg_vol
                )));
            }

            // Breakout Short: Price breaks below recent low
            if ctx.has_position
                && current_price < low * (Decimal::ONE - self.breakout_threshold_pct)
                && vol_ok
            {
                return Some(Signal::sell(format!(
                    "Bearish Breakout (Price={} < Low={}, Vol={} > Avg={})",
                    current_price, low, current_vol, avg_vol
                )));
            }
        }

        None
    }

    fn name(&self) -> &str {
        "Breakout"
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

    fn mock_candle(open: f64, high: f64, low: f64, close: f64, volume: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64(open).unwrap(),
            high: Decimal::from_f64(high).unwrap(),
            low: Decimal::from_f64(low).unwrap(),
            close: Decimal::from_f64(close).unwrap(),
            volume: Decimal::from_f64(volume).unwrap(),
            timestamp: 0,
        }
    }

    fn create_context(
        price: f64,
        candles: VecDeque<Candle>,
        has_position: bool,
    ) -> AnalysisContext {
        let d_price = Decimal::from_f64(price).unwrap();
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: d_price,
            price_f64: price,
            fast_sma: Decimal::ZERO,
            slow_sma: Decimal::ZERO,
            trend_sma: Decimal::ZERO,
            rsi: dec!(50.0),
            macd_value: Decimal::ZERO,
            macd_signal: Decimal::ZERO,
            macd_histogram: Decimal::ZERO,
            last_macd_histogram: None,
            atr: Decimal::ONE,
            bb_lower: Decimal::ZERO,
            bb_middle: Decimal::ZERO,
            bb_upper: Decimal::ZERO,
            adx: dec!(25.0),
            has_position,
            position: None,
            timestamp: 0,
            timeframe_features: None,
            candles,
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
    fn test_bullish_breakout() {
        let strategy = BreakoutStrategy::new(5, dec!(0.02), dec!(0.9)); // Use 0.9x volume (below avg)

        let mut candles = VecDeque::new();
        // Create a consolidation range with highest high = 105
        for _ in 0..5 {
            candles.push_back(mock_candle(98.0, 105.0, 95.0, 100.0, 1000.0));
        }

        // Add correct "current" context where current price is 110
        // The strategy now looks at previous 5 candles (the loop above) to find high=105
        // Current price 110 > 105 * 1.02 (107.1) -> Breakout
        // We need to add the current forming candle to the deque as well for the strategy to see "current volume"
        candles.push_back(mock_candle(108.0, 110.0, 109.0, 110.0, 1000.0));

        let ctx = create_context(110.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(
            sig.side,
            crate::domain::trading::types::OrderSide::Buy
        ));
        assert!(sig.reason.contains("Breakout"));
    }

    #[test]
    fn test_bearish_breakdown() {
        let strategy = BreakoutStrategy::new(5, dec!(0.02), dec!(0.9)); // Use 0.9x volume (below avg)

        let mut candles = VecDeque::new();
        // Create a range with lowest low = 95
        for _ in 0..5 {
            candles.push_back(mock_candle(98.0, 105.0, 95.0, 100.0, 1000.0));
        }

        // Add current candle for breakdown
        // Lowest low was 95. Breakdown threshold 95 * (1 - 0.02) = 93.1
        // Current price 92 < 93.1 -> Breakdown
        candles.push_back(mock_candle(93.0, 94.0, 92.0, 92.0, 1000.0));

        let ctx = create_context(92.0, candles, true);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(
            sig.side,
            crate::domain::trading::types::OrderSide::Sell
        ));
    }

    #[test]
    fn test_no_breakout_in_range() {
        let strategy = BreakoutStrategy::new(5, dec!(0.005), dec!(1.3));

        let mut candles = VecDeque::new();
        for _ in 0..5 {
            candles.push_back(mock_candle(98.0, 105.0, 95.0, 100.0, 1000.0));
        }

        // Add current candle within range
        candles.push_back(mock_candle(100.0, 102.0, 99.0, 102.0, 1000.0));

        // Price 102 is within the range
        let ctx = create_context(102.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none());
    }
}
