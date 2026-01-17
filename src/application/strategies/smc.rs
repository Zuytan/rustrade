use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::trading::types::{Candle, OrderSide};
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Smart Money Concepts (SMC) Strategy
///
/// Focuses on institutional footprints:
/// 1. Order Blocks (OB): Zones where significant buying/selling occurred.
/// 2. Fair Value Gaps (FVG): Imbalances in price action.
/// 3. Market Structure Shift (MSS): Changes in trend direction.
#[derive(Debug, Clone, Default)]
pub struct SMCStrategy {
    pub ob_lookback: usize,
    pub min_fvg_size_pct: f64,
    pub volume_multiplier: f64,
}

impl SMCStrategy {
    pub fn new(ob_lookback: usize, min_fvg_size_pct: f64, volume_multiplier: f64) -> Self {
        Self {
            ob_lookback,
            min_fvg_size_pct,
            volume_multiplier,
        }
    }

    /// Detect Fair Value Gaps (FVG)
    /// A bullish FVG is a gap between the High of Candle 1 and the Low of Candle 3,
    /// where Candle 2 is a large impulsive candle.
    ///
    /// Enhanced (v0.72): Checks if the FVG has been mitigated (filled) by subsequent price action.
    fn detect_fvg(&self, candles: &VecDeque<Candle>) -> Option<(OrderSide, f64)> {
        if candles.len() < 5 {
            // Need tracking capability
            return None;
        }

        // Look for FVG in the recent history (not just immediate last 3 candles)
        // We scan the last 10 candles for an UNMITIGATED FVG
        let scan_depth = 10.min(candles.len() - 3);
        let start_idx = candles.len() - scan_depth;

        for i in (start_idx..candles.len() - 2).rev() {
            // Start from most recent
            let c1 = &candles[i];
            let c3 = &candles[i + 2];

            let high1 = c1.high.to_f64().unwrap_or(0.0);
            let low1 = c1.low.to_f64().unwrap_or(0.0);
            let high3 = c3.high.to_f64().unwrap_or(0.0);
            let low3 = c3.low.to_f64().unwrap_or(0.0);

            // Bullish FVG: High of C1 < Low of C3 (Gap exists)
            if low3 > high1 {
                let gap = low3 - high1;
                let gap_pct = gap / high1;

                if gap_pct > self.min_fvg_size_pct {
                    // Check Mitigation: Has any candle AFTER the FVG (from i+3 onwards) dipped below low3?
                    // Actually, mitigation means price comes back to fill the gap.
                    // For a bullish FVG (gap up), we want to enter when price comes BACK DOWN into the gap.
                    // But if price goes BELOW high1 (the bottom of the gap), it's invalidated.

                    let fvg_top = low3;
                    let fvg_bottom = high1;

                    let mut invalidated = false;
                    let mut mitigated = false; // Is price currently IN the gap?

                    // Check subsequent candles
                    if i + 3 < candles.len() {
                        for j in (i + 3)..candles.len() {
                            let low = candles[j].low.to_f64().unwrap_or(0.0);
                            if low < fvg_bottom {
                                invalidated = true; // Price closed the gap completely and went lower
                                break;
                            }
                            if low < fvg_top {
                                mitigated = true; // Price is tapping into the gap - ENTRY SIGNAL
                            }
                        }
                    } else {
                        // FVG just formed (no subsequent candles yet)
                        // We can consider this a fresh FVG waiting for tap
                        mitigated = true; // Treat fresh FVG as actionable
                    }

                    if !invalidated && mitigated {
                        return Some((OrderSide::Buy, gap));
                    }
                }
            }

            // Bearish FVG: Low of C1 > High of C3
            if low1 > high3 {
                let gap = low1 - high3;
                let gap_pct = gap / high3;

                if gap_pct > self.min_fvg_size_pct {
                    let fvg_top = low1;
                    let fvg_bottom = high3;

                    let mut invalidated = false;
                    let mut mitigated = false;

                    if i + 3 < candles.len() {
                        for j in (i + 3)..candles.len() {
                            let high = candles[j].high.to_f64().unwrap_or(0.0);
                            if high > fvg_top {
                                invalidated = true; // Price went above top of bearish gap
                                break;
                            }
                            if high > fvg_bottom {
                                mitigated = true; // Price tapping into gap
                            }
                        }
                    } else {
                        mitigated = true;
                    }

                    if !invalidated && mitigated {
                        return Some((OrderSide::Sell, gap));
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
    fn find_last_ob(&self, candles: &VecDeque<Candle>, side: OrderSide) -> Option<f64> {
        // Lookback logic: look for opposite candle before a move
        if candles.len() < self.ob_lookback {
            return None;
        }

        // Calculate Average Volume for context
        let total_vol: f64 = candles
            .iter()
            .take(candles.len() - 1)
            .map(|c| c.volume)
            .sum();
        let avg_vol = total_vol / (candles.len() as f64 - 1.0);
        let vol_threshold = avg_vol * self.volume_multiplier;

        match side {
            OrderSide::Buy => {
                // Find last bearish candle followed by bullish candles
                for i in (1..candles.len() - 1).rev() {
                    let curr = &candles[i];
                    let next = &candles[i + 1]; // Impulsive candle

                    // Check structure: Bearish -> Bullish
                    if curr.close < curr.open && next.close > next.open {
                        // Volume Check: Next candle (impulsive) should have high volume
                        if next.volume > vol_threshold {
                            return Some(curr.low.to_f64().unwrap_or(0.0));
                        }
                    }
                }
            }
            OrderSide::Sell => {
                for i in (1..candles.len() - 1).rev() {
                    let curr = &candles[i];
                    let next = &candles[i + 1];
                    if curr.close > curr.open && next.close < next.open {
                        // Volume Check
                        if next.volume > vol_threshold {
                            return Some(curr.high.to_f64().unwrap_or(0.0));
                        }
                    }
                }
            }
        }
        None
    }

    /// Detect Market Structure Shift (MSS)
    /// A bullish MSS is confirmed when price closes above the last short-term high.
    fn detect_mss(&self, candles: &VecDeque<Candle>) -> Option<OrderSide> {
        if candles.len() < 10 {
            return None;
        }

        let curr_close = candles.back().unwrap().close.to_f64().unwrap_or(0.0);

        // Simplified MSS: check for break of recent 10-candle high/low
        let mut max_high = 0.0;
        let mut min_low = f64::MAX;

        for (i, _candle) in candles
            .iter()
            .enumerate()
            .take(candles.len() - 1)
            .skip(candles.len() - 10)
        {
            let h = candles[i].high.to_f64().unwrap_or(0.0);
            let l = candles[i].low.to_f64().unwrap_or(0.0);
            if h > max_high {
                max_high = h;
            }
            if l < min_low {
                min_low = l;
            }
        }

        if curr_close > max_high {
            return Some(OrderSide::Buy);
        } else if curr_close < min_low {
            return Some(OrderSide::Sell);
        }

        None
    }
}

impl TradingStrategy for SMCStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let fvg = self.detect_fvg(&ctx.candles);
        let mss = self.detect_mss(&ctx.candles);

        if let Some((side, _gap)) = fvg {
            let ob = self.find_last_ob(&ctx.candles, side);

            match side {
                OrderSide::Buy => {
                    // Bullish Bias if MSS is bullish or price is above SMA
                    let structure_bullish =
                        mss == Some(OrderSide::Buy) || ctx.price_f64 > ctx.trend_sma;

                    if structure_bullish {
                        let reason = if let Some(ob_level) = ob {
                            format!(
                                "SMC: Bullish FVG detected with OB at {:.2}. Structure is bullish.",
                                ob_level
                            )
                        } else {
                            "SMC: Bullish FVG detected. Structure is bullish.".to_string()
                        };
                        return Some(Signal::buy(reason).with_confidence(0.85));
                    }
                }
                OrderSide::Sell => {
                    let structure_bearish =
                        mss == Some(OrderSide::Sell) || ctx.price_f64 < ctx.trend_sma;

                    if structure_bearish {
                        let reason = if let Some(ob_level) = ob {
                            format!(
                                "SMC: Bearish FVG detected with OB at {:.2}. Structure is bearish.",
                                ob_level
                            )
                        } else {
                            "SMC: Bearish FVG detected. Structure is bearish.".to_string()
                        };
                        return Some(Signal::sell(reason).with_confidence(0.85));
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
            volume,
            timestamp: 0,
        }
    }

    #[test]
    fn test_bullish_fvg_detection() {
        let strategy = SMCStrategy::new(20, 0.001, 1.5);
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

        // C4: Mitigation candle (dips into gap but valid)
        // Gap is between 102.0 (High1) and 105.0 (Low3)
        // C4 Low = 103.0 (Inside gap) -> Mitigated!
        candles.push_back(mock_candle(111.0, 112.0, 103.0, 111.0, 1000.0));

        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_some());
        let (side, gap) = fvg.unwrap();
        assert_eq!(side, OrderSide::Buy);
        assert_eq!(gap, 3.0);
    }

    #[test]
    fn test_bearish_fvg_detection() {
        let strategy = SMCStrategy::new(20, 0.001, 1.5);
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

        // C4: Mitigation
        // Gap is between 98.0 (Low1) and 95.0 (High3)
        // C4 High = 96.0 (Inside gap) -> Mitigated!
        candles.push_back(mock_candle(90.0, 96.0, 88.0, 89.0, 1000.0));

        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_some());
        let (side, gap) = fvg.unwrap();
        assert_eq!(side, OrderSide::Sell);
        assert_eq!(gap, 3.0);
    }

    #[test]
    fn test_ob_detection() {
        let strategy = SMCStrategy::new(20, 0.001, 1.2); // 1.2x volume multiplier
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
        assert_eq!(ob.unwrap(), 100.0);
    }

    #[test]
    fn test_ob_detection_fails_low_volume() {
        let strategy = SMCStrategy::new(20, 0.001, 1.5); // 1.5x volume multiplier
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
        let strategy = SMCStrategy::new(20, 0.001, 1.0);
        let mut candles = VecDeque::new();

        // Padding
        for _ in 0..10 {
            candles.push_back(mock_candle(100.0, 100.0, 100.0, 100.0, 1000.0));
        }

        // FVG Setup
        candles.push_back(mock_candle(100.0, 102.0, 99.0, 101.0, 1000.0)); // High 102
        candles.push_back(mock_candle(101.0, 110.0, 101.0, 109.0, 1000.0));
        candles.push_back(mock_candle(109.0, 112.0, 105.0, 111.0, 1000.0)); // Low 105
        // Gap: 102-105

        // Invalidation: Price drops BELOW 102
        candles.push_back(mock_candle(111.0, 112.0, 101.0, 102.0, 1000.0)); // Low 101 < 102

        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_none(), "FVG should be invalidated");
    }

    #[test]
    fn test_mss_detection() {
        let strategy = SMCStrategy::new(20, 0.001, 1.0);
        let mut candles = VecDeque::new();

        // Add 9 candles with high around 110
        for _i in 0..9 {
            candles.push_back(mock_candle(100.0, 110.0, 90.0, 105.0, 1000.0));
        }
        // 10th candle breaks high
        candles.push_back(mock_candle(110.0, 115.0, 110.0, 114.0, 1000.0));

        let mss = strategy.detect_mss(&candles);
        assert_eq!(mss, Some(OrderSide::Buy));
    }
}
