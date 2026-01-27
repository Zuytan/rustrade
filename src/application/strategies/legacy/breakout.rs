use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::prelude::ToPrimitive;

/// Breakout Strategy
///
/// Detects breakouts from consolidation patterns by monitoring:
/// - Price breaking above recent highs (bullish breakout)
/// - Price breaking below recent lows (bearish breakout)
/// - Optional volume confirmation for stronger signals
#[derive(Debug, Clone)]
pub struct BreakoutStrategy {
    pub lookback_period: usize,      // Period to calculate high/low
    pub breakout_threshold_pct: f64, // % above/below high/low to confirm breakout
    pub volume_multiplier: f64,      // Required volume vs average (e.g., 1.5 = 50% above avg)
}

impl BreakoutStrategy {
    pub fn new(
        lookback_period: usize,
        breakout_threshold_pct: f64,
        volume_multiplier: f64,
    ) -> Self {
        Self {
            lookback_period,
            breakout_threshold_pct,
            volume_multiplier,
        }
    }

    /// Calculate recent high and low from candle history
    fn calculate_range(&self, ctx: &AnalysisContext) -> Option<(f64, f64, f64)> {
        if ctx.candles.len() < self.lookback_period {
            return None;
        }

        let start_idx = ctx.candles.len().saturating_sub(self.lookback_period);

        let mut highest_high = f64::MIN;
        let mut lowest_low = f64::MAX;
        let mut total_volume = 0.0;
        let mut count = 0;

        for candle in ctx.candles.iter().skip(start_idx) {
            let high = candle.high.to_f64().unwrap_or(0.0);
            let low = candle.low.to_f64().unwrap_or(0.0);

            if high > highest_high {
                highest_high = high;
            }
            if low < lowest_low {
                lowest_low = low;
            }
            total_volume += candle.volume.to_f64().unwrap_or(0.0);
            count += 1;
        }

        let avg_volume = if count > 0 {
            total_volume / count as f64
        } else {
            0.0
        };

        Some((highest_high, lowest_low, avg_volume))
    }
}

impl Default for BreakoutStrategy {
    fn default() -> Self {
        Self {
            lookback_period: 10,           // Reduced from 20 for faster detection
            breakout_threshold_pct: 0.002, // 0.2% above high (reduced from 0.5%)
            volume_multiplier: 1.1,        // 10% above average (reduced from 30%)
        }
    }
}

impl TradingStrategy for BreakoutStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if let Some((high, low, avg_vol)) = self.calculate_range(ctx) {
            let current_vol = ctx.candles.back()?.volume;
            let current_price = ctx.price_f64;

            let vol_ok = current_vol.to_f64().unwrap_or(0.0) >= avg_vol * self.volume_multiplier;

            // Breakout Long: Price breaks above recent high
            if current_price > high * (1.0 + self.breakout_threshold_pct) && vol_ok {
                return Some(Signal::buy(format!(
                    "Bullish Breakout (Price={:.2} > High={:.2}, Vol={:.0} > Avg={:.0})",
                    current_price, high, current_vol, avg_vol
                )));
            }

            // Breakout Short: Price breaks below recent low
            if current_price < low * (1.0 - self.breakout_threshold_pct) && vol_ok {
                return Some(Signal::sell(format!(
                    "Bearish Breakout (Price={:.2} < Low={:.2}, Vol={:.0} > Avg={:.0})",
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
        use rust_decimal_macros::dec;
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
            price_f64: price,
            fast_sma: 0.0,
            slow_sma: 0.0,
            trend_sma: 0.0,
            rsi: 50.0,
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
            rsi_history: VecDeque::new(),
            // OFI fields (defaults for tests)
            ofi_value: 0.0,
            cumulative_delta: 0.0,
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
        let strategy = BreakoutStrategy::new(5, 0.02, 0.9); // Use 0.9x volume (below avg)

        let mut candles = VecDeque::new();
        // Create a consolidation range with highest high = 105
        for _ in 0..5 {
            candles.push_back(mock_candle(98.0, 105.0, 95.0, 100.0, 1000.0));
        }

        // Price 110 > 105 * 1.02 = 107.1 -> breakout
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
        let strategy = BreakoutStrategy::new(5, 0.02, 0.9); // Use 0.9x volume (below avg)

        let mut candles = VecDeque::new();
        // Create a range with lowest low = 95
        for _ in 0..5 {
            candles.push_back(mock_candle(98.0, 105.0, 95.0, 100.0, 1000.0));
        }

        // Price 92 < 95 * 0.98 = 93.1 -> breakdown
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
        let strategy = BreakoutStrategy::new(5, 0.005, 1.3);

        let mut candles = VecDeque::new();
        for _ in 0..5 {
            candles.push_back(mock_candle(98.0, 105.0, 95.0, 100.0, 1000.0));
        }

        // Price 102 is within the range
        let ctx = create_context(102.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none());
    }
}
