use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::trading::types::{Candle, OrderSide};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Smart Money Concepts (SMC) Strategy
///
/// Focuses on institutional footprints:
/// 1. Order Blocks (OB): Zones where significant buying/selling occurred.
/// 2. Fair Value Gaps (FVG): Imbalances in price action.
/// 3. Market Structure Shift (MSS): Changes in trend direction.
#[derive(Debug, Clone)]
pub struct SMCStrategy {
    pub ob_lookback: usize,
    pub min_fvg_size_pct: Decimal,
    pub volume_multiplier: Decimal,
    /// Minimum OFI required for signal confirmation.
    /// Set to 0.0 to disable OFI gating (useful when OFI data is unreliable).
    pub ofi_threshold: Decimal,
}

impl Default for SMCStrategy {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            ob_lookback: 20,
            min_fvg_size_pct: dec!(0.005),
            volume_multiplier: dec!(1.5),
            ofi_threshold: dec!(0.0), // Disabled by default for crypto compatibility
        }
    }
}

impl SMCStrategy {
    pub fn new(ob_lookback: usize, min_fvg_size_pct: Decimal, volume_multiplier: Decimal) -> Self {
        Self {
            ob_lookback,
            min_fvg_size_pct,
            volume_multiplier,
            ofi_threshold: Decimal::ZERO, // Disabled by default for crypto compatibility
        }
    }

    /// Detect Fair Value Gaps (FVG)
    /// A bullish FVG is a gap between the High of Candle 1 and the Low of Candle 3,
    /// where Candle 2 is a large impulsive candle.
    ///
    /// Returns: Option<(Side, GapSize, InvalidationLevel)>
    pub(crate) fn detect_fvg(
        &self,
        candles: &VecDeque<Candle>,
    ) -> Option<(OrderSide, Decimal, Decimal)> {
        if candles.len() < 5 {
            return None;
        }

        // Look for FVG in the recent history
        let scan_depth = 20.min(candles.len() - 3);
        let start_idx = candles.len() - scan_depth;

        // Iterate RECENT to OLD (finding the most relevant recent structure)
        for i in (start_idx..candles.len() - 2).rev() {
            let c1 = &candles[i];
            let c2 = &candles[i + 1];
            let c3 = &candles[i + 2];

            let high1 = c1.high;
            let low1 = c1.low;
            let high3 = c3.high;
            let low3 = c3.low;

            // Bullish FVG: gap between C1 high and C3 low
            if low3 > high1 {
                // C2 MUST be an impulsive bullish candle (close > open, large body)
                let c2_body = c2.close - c2.open;
                if c2_body <= Decimal::ZERO {
                    continue; // C2 is not bullish — skip
                }
                let c2_body_abs = c2_body.abs();
                if c2_body_abs < self.min_fvg_size_pct * c2.open {
                    continue; // C2 body too small to be impulsive
                }

                let gap = low3 - high1;
                let midpoint = (high1 + low3) / Decimal::TWO;
                let gap_pct = if midpoint > Decimal::ZERO {
                    gap / midpoint
                } else {
                    Decimal::ZERO
                };

                if gap_pct > self.min_fvg_size_pct {
                    let fvg_bottom = high1;
                    let fvg_top = low3;

                    let mut invalidated = false;
                    let mut in_zone = false;

                    // Check all subsequent candles for invalidation or entry
                    // We start from i+3 (candle AFTER the FVG formation)
                    for (idx, candle) in candles.iter().enumerate().skip(i + 3) {
                        let low = candle.low;

                        // Strict invalidation: if price closes gap completely
                        if low < fvg_bottom {
                            invalidated = true;
                            break;
                        }

                        // Check if CURRENT candle (the last one) is in zone
                        if idx == candles.len() - 1 {
                            // Entry condition: Price TOUCHED zone (Low <= Top)
                            if candle.low <= fvg_top {
                                in_zone = true;
                            }
                        }
                    }

                    if !invalidated && in_zone {
                        return Some((OrderSide::Buy, gap, fvg_bottom));
                    }
                }
            }

            // Bearish FVG: Low1 > High3
            if low1 > high3 {
                // C2 MUST be an impulsive bearish candle (close < open, large body)
                let c2_body = c2.open - c2.close;
                if c2_body <= Decimal::ZERO {
                    continue; // C2 is not bearish — skip
                }
                let c2_body_abs = c2_body.abs();
                if c2_body_abs < self.min_fvg_size_pct * c2.open {
                    continue; // C2 body too small to be impulsive
                }

                let gap = low1 - high3;
                let midpoint = (low1 + high3) / Decimal::TWO;
                let gap_pct = if midpoint > Decimal::ZERO {
                    gap / midpoint
                } else {
                    Decimal::ZERO
                };

                if gap_pct > self.min_fvg_size_pct {
                    let fvg_top = low1;
                    let fvg_bottom = high3;

                    let mut invalidated = false;
                    let mut in_zone = false;

                    for (idx, candle) in candles.iter().enumerate().skip(i + 3) {
                        let high = candle.high;

                        if high > fvg_top {
                            invalidated = true;
                            break;
                        }

                        if idx == candles.len() - 1 {
                            // Entry condition: Price TOUCHED zone (High >= Bottom)
                            if candle.high >= fvg_bottom {
                                in_zone = true;
                            }
                        }
                    }

                    if !invalidated && in_zone {
                        return Some((OrderSide::Sell, gap, fvg_top));
                    }
                }
            }
        }

        None
    }

    /// Detect Order Blocks (OB)
    /// A bullish OB is the last bearish candle before a strong impulsive bullish move.
    ///
    /// Enhanced (v0.72): Requires Volume Confirmation. The impulsive move must have volume > average.
    fn find_last_ob(&self, candles: &VecDeque<Candle>, side: OrderSide) -> Option<Decimal> {
        // Lookback logic: look for opposite candle before a move
        if candles.len() < self.ob_lookback {
            return None;
        }

        // Calculate Average Volume for context (Last 50 candles max)
        let vol_lookback = 50.min(candles.len().saturating_sub(1));
        let total_vol: Decimal = candles
            .iter()
            .rev()
            .skip(1) // Skip current candle (incomplete)
            .take(vol_lookback)
            .map(|c| c.volume)
            .sum();

        let avg_vol = if vol_lookback > 0 {
            total_vol / Decimal::from(vol_lookback)
        } else {
            Decimal::ZERO
        };
        let vol_threshold = avg_vol * self.volume_multiplier;

        // Limit search depth to ob_lookback
        let start_index = candles.len().saturating_sub(self.ob_lookback).max(1);

        match side {
            OrderSide::Buy => {
                // Find last bearish candle followed by bullish candles
                for i in (start_index..candles.len() - 1).rev() {
                    let curr = &candles[i];
                    let next = &candles[i + 1]; // Impulsive candle

                    // Check structure: Bearish -> Bullish
                    if curr.close < curr.open && next.close > next.open {
                        // Volume Check: Next candle (impulsive) should have high volume
                        if next.volume > vol_threshold {
                            return Some(curr.low);
                        }
                    }
                }
            }
            OrderSide::Sell => {
                for i in (start_index..candles.len() - 1).rev() {
                    let curr = &candles[i];
                    let next = &candles[i + 1];
                    if curr.close > curr.open && next.close < next.open {
                        // Volume Check
                        if next.volume > vol_threshold {
                            return Some(curr.high);
                        }
                    }
                }
            }
        }
        None
    }

    /// Check if a candle is a Swing High (Fractal High)
    /// A Swing High is higher than N candles before and N candles after.
    fn is_swing_high(&self, candles: &VecDeque<Candle>, index: usize, range: usize) -> bool {
        if index < range || index >= candles.len() - range {
            return false;
        }
        let high = candles[index].high;

        // Check left side
        for i in 1..=range {
            if candles[index - i].high >= high {
                return false;
            }
        }
        // Check right side
        for i in 1..=range {
            if candles[index + i].high > high {
                // Strict inequality on right side to avoid double detection
                return false;
            }
        }
        true
    }

    /// Check if a candle is a Swing Low (Fractal Low)
    /// A Swing Low is lower than N candles before and N candles after.
    fn is_swing_low(&self, candles: &VecDeque<Candle>, index: usize, range: usize) -> bool {
        if index < range || index >= candles.len() - range {
            return false;
        }
        let low = candles[index].low;

        // Check left side
        for i in 1..=range {
            if candles[index - i].low <= low {
                return false;
            }
        }
        // Check right side
        for i in 1..=range {
            if candles[index + i].low < low {
                // Strict inequality on right side
                return false;
            }
        }
        true
    }

    /// Detect recent meaningful Swing Points (Highs and Lows)
    /// Returns the most recent Swing High and Swing Low prices found in the lookback period.
    fn detect_recent_swing_points(
        &self,
        candles: &VecDeque<Candle>,
    ) -> (Option<Decimal>, Option<Decimal>) {
        let fractal_range = 3; // Standard Williams Fractal uses 2, we use 3 for more significance
        let lookback = 50.min(candles.len().saturating_sub(fractal_range));

        // Use a persistent notion of structure:
        // We want the LAST significant swing point before the current price action.
        // So we iterate backwards from (current - fractal_range)

        let mut last_swing_high = None;
        let mut last_swing_low = None;

        let start_index = candles.len().saturating_sub(fractal_range + 1);
        let end_index = candles.len().saturating_sub(lookback);

        // Scan backwards to find the most recent confirmed swings
        for i in (end_index..=start_index).rev() {
            if last_swing_high.is_none() && self.is_swing_high(candles, i, fractal_range) {
                last_swing_high = Some(candles[i].high);
            }
            if last_swing_low.is_none() && self.is_swing_low(candles, i, fractal_range) {
                last_swing_low = Some(candles[i].low);
            }
            if last_swing_high.is_some() && last_swing_low.is_some() {
                break;
            }
        }

        (last_swing_high, last_swing_low)
    }

    /// Detect Market Structure Shift (MSS)
    /// A Bullish MSS occurs when price breaks and closes ABOVE the last valid Swing High.
    /// A Bearish MSS occurs when price breaks and closes BELOW the last valid Swing Low.
    fn detect_mss(&self, candles: &VecDeque<Candle>) -> Option<OrderSide> {
        if candles.len() < 20 {
            return None;
        }

        let curr_close = candles.back()?.close;
        let (recent_high, recent_low) = self.detect_recent_swing_points(candles);

        if recent_high.is_some_and(|swing_high| curr_close > swing_high) {
            return Some(OrderSide::Buy);
        }

        if recent_low.is_some_and(|swing_low| curr_close < swing_low) {
            return Some(OrderSide::Sell);
        }

        None
    }
}

impl TradingStrategy for SMCStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        use rust_decimal_macros::dec;
        let fvg = self.detect_fvg(&ctx.candles);
        let mss = self.detect_mss(&ctx.candles);

        if let Some((side, _gap, invalidation_level)) = fvg {
            let ob = self.find_last_ob(&ctx.candles, side);

            match side {
                OrderSide::Buy => {
                    // Bullish Bias if MSS is bullish or price is above SMA
                    // Check trend_sma safely
                    let trend_bullish = if let Some(trend) = ctx.trend_sma {
                        ctx.current_price > trend
                    } else {
                        false // Fallback or strict mode? Let's say false (no trend conf)
                    };

                    let structure_bullish = mss == Some(OrderSide::Buy) || trend_bullish;

                    if structure_bullish {
                        // OFI Validation: Require positive OFI for bullish signals (when threshold > 0)
                        // This confirms institutional buying pressure
                        if self.ofi_threshold > Decimal::ZERO && ctx.ofi_value < self.ofi_threshold
                        {
                            tracing::debug!(
                                "SMC [{}]: Bullish FVG blocked - Weak OFI ({} < {})",
                                ctx.symbol,
                                ctx.ofi_value,
                                self.ofi_threshold
                            );
                            return None;
                        }

                        // Cumulative Delta Confirmation (optional, increases confidence)
                        let delta_confirms = ctx.cumulative_delta > Decimal::ZERO;
                        let confidence = if delta_confirms && ob.is_some() {
                            0.95 // OFI + Delta + OB = highest confidence
                        } else if delta_confirms || ob.is_some() {
                            0.90 // OFI + (Delta OR OB)
                        } else {
                            0.85 // OFI only
                        };

                        let reason = if let Some(ob_level) = ob {
                            format!(
                                "SMC: Bullish FVG + OB at {} (OFI={}, Delta={})",
                                ob_level, ctx.ofi_value, ctx.cumulative_delta
                            )
                        } else {
                            format!(
                                "SMC: Bullish FVG (OFI={}, Delta={})",
                                ctx.ofi_value, ctx.cumulative_delta
                            )
                        };

                        let mut signal = Signal::buy(reason).with_confidence(confidence);

                        // Stop Loss Strategy:
                        // 1. If an Order Block (OB) exists, place stop below the OB (standard SMC).
                        // 2. If no OB, place stop below the FVG invalidation level.
                        if let Some(ob_level) = ob {
                            // If OB exists, stop goes below OB
                            signal = signal.with_stop_loss(ob_level * dec!(0.999)); // 0.1% buffer
                        } else {
                            // Stop below FVG invalidation level
                            signal = signal.with_stop_loss(invalidation_level * dec!(0.999));
                        }

                        return Some(signal);
                    }
                }
                OrderSide::Sell => {
                    let trend_bearish = if let Some(trend) = ctx.trend_sma {
                        ctx.current_price < trend
                    } else {
                        false
                    };

                    let structure_bearish = mss == Some(OrderSide::Sell) || trend_bearish;

                    if structure_bearish {
                        // OFI Validation: Require negative OFI for bearish signals (when threshold > 0)
                        // This confirms institutional selling pressure
                        if self.ofi_threshold > Decimal::ZERO && ctx.ofi_value > -self.ofi_threshold
                        {
                            tracing::debug!(
                                "SMC [{}]: Bearish FVG blocked - Weak OFI ({} > -{})",
                                ctx.symbol,
                                ctx.ofi_value,
                                self.ofi_threshold
                            );
                            return None;
                        }

                        // Cumulative Delta Confirmation
                        let delta_confirms = ctx.cumulative_delta < Decimal::ZERO;
                        let confidence = if delta_confirms && ob.is_some() {
                            0.95 // OFI + Delta + OB = highest confidence
                        } else if delta_confirms || ob.is_some() {
                            0.90 // OFI + (Delta OR OB)
                        } else {
                            0.85 // OFI only
                        };

                        let reason = if let Some(ob_level) = ob {
                            format!(
                                "SMC: Bearish FVG + OB at {} (OFI={}, Delta={})",
                                ob_level, ctx.ofi_value, ctx.cumulative_delta
                            )
                        } else {
                            format!(
                                "SMC: Bearish FVG (OFI={}, Delta={})",
                                ctx.ofi_value, ctx.cumulative_delta
                            )
                        };

                        let mut signal = Signal::sell(reason).with_confidence(confidence);

                        // Stop Loss: Just above FVG top (Low1) or OB High
                        if let Some(ob_level) = ob {
                            signal = signal.with_stop_loss(ob_level * dec!(1.001));
                        } else {
                            signal = signal.with_stop_loss(invalidation_level * dec!(1.001));
                        }

                        return Some(signal);
                    }
                }
            }
        }

        None
    }

    fn name(&self) -> &str {
        "SMC"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::Candle;
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromPrimitive;

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

    #[test]
    fn test_bullish_fvg_detection() {
        use rust_decimal_macros::dec;
        let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.5));
        let mut candles = VecDeque::new();

        // Padding candles to satisfy length check (need 5)
        for _ in 0..10 {
            candles.push_back(mock_candle(100.0, 101.0, 99.0, 100.0, 1000.0));
        }

        // C1: Small candle
        candles.push_back(mock_candle(100.0, 102.0, 99.0, 101.0, 1000.0));
        // C2: Big impulsive candle
        candles.push_back(mock_candle(101.0, 110.0, 101.0, 109.0, 2000.0));
        // C3: Follow through
        candles.push_back(mock_candle(109.0, 112.0, 105.0, 111.0, 1000.0));

        // C4: Mitigation candle - Close must be in gap for entry
        // Gap is between 102.0 (High1) and 105.0 (Low3)
        // C4 Close = 104.0 (Inside gap) -> Valid entry!
        candles.push_back(mock_candle(111.0, 112.0, 103.0, 104.0, 1000.0));

        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_some());
        let (side, gap, invalidation) = fvg.unwrap();
        assert_eq!(side, OrderSide::Buy);
        assert_eq!(gap, dec!(3.0));
        assert_eq!(invalidation, dec!(102.0)); // High1
    }

    #[test]
    fn test_bearish_fvg_detection() {
        use rust_decimal_macros::dec;
        let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.5));
        let mut candles = VecDeque::new();

        // Padding
        for _ in 0..10 {
            candles.push_back(mock_candle(100.0, 101.0, 99.0, 100.0, 1000.0));
        }

        // C1: Small candle
        candles.push_back(mock_candle(100.0, 101.0, 98.0, 99.0, 1000.0));
        // C2: Big impulsive candle
        candles.push_back(mock_candle(99.0, 99.0, 90.0, 91.0, 2000.0));
        // C3: Follow through
        candles.push_back(mock_candle(91.0, 95.0, 89.0, 90.0, 1000.0));

        // C4: Mitigation candle - Close must be in gap for entry
        // Gap is between 98.0 (Low1) and 95.0 (High3)
        // C4 Close = 96.5 (Inside gap) -> Valid entry!
        candles.push_back(mock_candle(90.0, 96.0, 88.0, 96.5, 1000.0));

        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_some());
        let (side, gap, invalidation) = fvg.unwrap();
        assert_eq!(side, OrderSide::Sell);
        assert_eq!(gap, dec!(3.0));
        assert_eq!(invalidation, dec!(98.0)); // Low1
    }

    #[test]
    fn test_ob_detection() {
        use rust_decimal_macros::dec;
        let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.2)); // 1.2x volume multiplier
        let mut candles = VecDeque::new();

        // Padding to satisfy OB Lookback (20)
        for _ in 0..20 {
            candles.push_back(mock_candle(100.0, 101.0, 99.0, 100.5, 1000.0));
        }

        // Add 5 candles setup
        candles.push_back(mock_candle(100.0, 101.0, 99.0, 100.5, 1000.0));
        candles.push_back(mock_candle(100.5, 102.0, 100.0, 101.5, 1000.0));
        candles.push_back(mock_candle(101.5, 103.0, 101.0, 102.0, 1000.0));
        // Bearish candle (Potential OB) - Average volume so far ~1000
        candles.push_back(mock_candle(102.0, 102.5, 100.0, 100.5, 1000.0));
        // Followed by Bullish candle with HIGH VOLUME
        candles.push_back(mock_candle(101.0, 105.0, 101.0, 104.0, 1500.0)); // 1.5x avg

        let ob = strategy.find_last_ob(&candles, OrderSide::Buy);
        assert!(
            ob.is_some(),
            "Should detect OB because volume is high enough"
        );
        assert_eq!(ob.unwrap(), dec!(100.0));
    }

    #[test]
    fn test_ob_detection_fails_low_volume() {
        use rust_decimal_macros::dec;
        let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.5)); // 1.5x volume multiplier
        let mut candles = VecDeque::new();

        // Add context candles
        for _ in 0..10 {
            candles.push_back(mock_candle(100.0, 101.0, 99.0, 100.5, 1000.0));
        }

        // Bearish candle (Potential OB)
        candles.push_back(mock_candle(102.0, 102.5, 100.0, 100.5, 1000.0));
        // Followed by Bullish candle with LOW VOLUME
        candles.push_back(mock_candle(101.0, 105.0, 101.0, 104.0, 1100.0)); // Only 1.1x avg, need 1.5x

        let ob = strategy.find_last_ob(&candles, OrderSide::Buy);
        assert!(
            ob.is_none(),
            "Should NOT detect OB due to insufficient volume"
        );
    }

    #[test]
    fn test_fvg_invalidation() {
        use rust_decimal_macros::dec;
        let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.0));
        let mut candles = VecDeque::new();

        // Padding
        for _ in 0..10 {
            candles.push_back(mock_candle(100.0, 100.0, 100.0, 100.0, 1000.0));
        }

        // FVG Setup
        candles.push_back(mock_candle(100.0, 102.0, 99.0, 101.0, 1000.0)); // High 102
        // C2: Big impulsive candle (Low 100.0 to avoid accidental gap with padding)
        candles.push_back(mock_candle(101.0, 110.0, 100.0, 109.0, 1000.0));
        candles.push_back(mock_candle(109.0, 112.0, 105.0, 111.0, 1000.0)); // Low 105
        // Gap: 102-105

        // Invalidation: Price drops BELOW 102
        candles.push_back(mock_candle(111.0, 112.0, 101.0, 102.0, 1000.0)); // Low 101 < 102

        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_none(), "FVG should be invalidated");
    }

    #[test]
    fn test_mss_detection() {
        use rust_decimal_macros::dec;
        let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.0));
        let mut candles = VecDeque::new();

        // Setup a Swing High pattern
        // Fractal Range = 3. We need 3 lower, 1 high, 3 lower.

        let base_price = 100.0;

        // 1. Pre-Swing ramp up
        for i in 0..10 {
            candles.push_back(mock_candle(
                base_price + i as f64,
                base_price + i as f64 + 1.0,
                base_price + i as f64 - 1.0,
                base_price + i as f64,
                1000.0,
            ));
        }

        // 2. Form Swing High at 120.0
        // Left side (ascending/lower highs) - already in loop above mostly, but let's be explicit for the fractal range
        candles.push_back(mock_candle(115.0, 118.0, 114.0, 117.0, 1000.0)); // i-3
        candles.push_back(mock_candle(117.0, 119.0, 116.0, 118.0, 1000.0)); // i-2
        candles.push_back(mock_candle(118.0, 119.5, 117.0, 119.0, 1000.0)); // i-1

        // Peak
        candles.push_back(mock_candle(119.0, 120.0, 118.0, 119.0, 1000.0)); // i (High 120)

        // Right side (lower highs) - critical for confirming the Swing High
        candles.push_back(mock_candle(119.0, 119.5, 118.0, 118.5, 1000.0)); // i+1
        candles.push_back(mock_candle(118.5, 119.0, 117.0, 118.0, 1000.0)); // i+2
        candles.push_back(mock_candle(118.0, 118.5, 116.0, 117.0, 1000.0)); // i+3 -> Swing High confirmed here!

        // 3. Pullback
        candles.push_back(mock_candle(117.0, 118.0, 116.0, 116.5, 1000.0));
        candles.push_back(mock_candle(116.5, 117.0, 115.0, 116.0, 1000.0));

        // 4. Break of Structure (MSS) - Close above 120.0
        candles.push_back(mock_candle(120.0, 121.0, 119.0, 120.5, 1000.0));

        let mss = strategy.detect_mss(&candles);
        assert_eq!(
            mss,
            Some(OrderSide::Buy),
            "Should detect Bullish MSS on close above Swing High (120.0)"
        );
    }
}
