use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::trading::types::{OrderSide, Candle};
use std::collections::VecDeque;
use rust_decimal::prelude::ToPrimitive;

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
}

impl SMCStrategy {
    pub fn new(ob_lookback: usize, min_fvg_size_pct: f64) -> Self {
        Self {
            ob_lookback,
            min_fvg_size_pct,
        }
    }

    /// Detect Fair Value Gaps (FVG)
    /// A bullish FVG is a gap between the High of Candle 1 and the Low of Candle 3, 
    /// where Candle 2 is a large impulsive candle.
    fn detect_fvg(&self, candles: &VecDeque<Candle>) -> Option<(OrderSide, f64)> {
        if candles.len() < 3 {
            return None;
        }

        let c1 = &candles[candles.len() - 3];
        let c3 = &candles[candles.len() - 1];

        let high1 = c1.high.to_f64().unwrap_or(0.0);
        let low3 = c3.low.to_f64().unwrap_or(0.0);
        
        // Bullish FVG: High of C1 < Low of C3 (Gap exists)
        if low3 > high1 {
            let gap = low3 - high1;
            let gap_pct = gap / high1;
            if gap_pct > self.min_fvg_size_pct {
                return Some((OrderSide::Buy, gap));
            }
        }

        let low1 = c1.low.to_f64().unwrap_or(0.0);
        let high3 = c3.high.to_f64().unwrap_or(0.0);

        // Bearish FVG: Low of C1 > High of C3
        if low1 > high3 {
            let gap = low1 - high3;
            let gap_pct = gap / high3;
            if gap_pct > self.min_fvg_size_pct {
                return Some((OrderSide::Sell, gap));
            }
        }

        None
    }

    /// Detect Order Blocks (OB)
    /// A bullish OB is the last bearish candle before a strong impulsive bullish move.
    fn find_last_ob(&self, candles: &VecDeque<Candle>, side: OrderSide) -> Option<f64> {
        // Simplified OB detection: look for opposite candle before a move
        if candles.len() < 5 {
            return None;
        }

        match side {
            OrderSide::Buy => {
                // Find last bearish candle followed by bullish candles
                for i in (1..candles.len() - 1).rev() {
                    let curr = &candles[i];
                    let next = &candles[i+1];
                    if curr.close < curr.open && next.close > next.open {
                        return Some(curr.low.to_f64().unwrap_or(0.0));
                    }
                }
            }
            OrderSide::Sell => {
                 for i in (1..candles.len() - 1).rev() {
                    let curr = &candles[i];
                    let next = &candles[i+1];
                    if curr.close > curr.open && next.close < next.open {
                        return Some(curr.high.to_f64().unwrap_or(0.0));
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

        for i in (candles.len() - 10)..(candles.len() - 1) {
            let h = candles[i].high.to_f64().unwrap_or(0.0);
            let l = candles[i].low.to_f64().unwrap_or(0.0);
            if h > max_high { max_high = h; }
            if l < min_low { min_low = l; }
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
                    let structure_bullish = mss == Some(OrderSide::Buy) || ctx.price_f64 > ctx.trend_sma;
                    
                    if structure_bullish {
                         let reason = if let Some(ob_level) = ob {
                             format!("SMC: Bullish FVG detected with OB at {:.2}. Structure is bullish.", ob_level)
                         } else {
                             "SMC: Bullish FVG detected. Structure is bullish.".to_string()
                         };
                         return Some(Signal::buy(reason).with_confidence(0.85));
                    }
                }
                OrderSide::Sell => {
                    let structure_bearish = mss == Some(OrderSide::Sell) || ctx.price_f64 < ctx.trend_sma;

                    if structure_bearish {
                        let reason = if let Some(ob_level) = ob {
                             format!("SMC: Bearish FVG detected with OB at {:.2}. Structure is bearish.", ob_level)
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

    fn mock_candle(open: f64, high: f64, low: f64, close: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64(open).unwrap(),
            high: Decimal::from_f64(high).unwrap(),
            low: Decimal::from_f64(low).unwrap(),
            close: Decimal::from_f64(close).unwrap(),
            volume: 1000.0,
            timestamp: 0,
        }
    }

    #[test]
    fn test_bullish_fvg_detection() {
        let strategy = SMCStrategy::new(20, 0.001);
        let mut candles = VecDeque::new();
        
        // C1: Small candle
        candles.push_back(mock_candle(100.0, 102.0, 99.0, 101.0));
        // C2: Big impulsive candle (should be bigger but FVG is between C1 high and C3 low)
        candles.push_back(mock_candle(101.0, 110.0, 101.0, 109.0));
        // C3: Follow through
        candles.push_back(mock_candle(109.0, 112.0, 105.0, 111.0));

        // High of C1 is 102.0, Low of C3 is 105.0. Gap of 3.0.
        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_some());
        let (side, gap) = fvg.unwrap();
        assert_eq!(side, OrderSide::Buy);
        assert_eq!(gap, 3.0);
    }
    
    #[test]
    fn test_bearish_fvg_detection() {
        let strategy = SMCStrategy::new(20, 0.001);
        let mut candles = VecDeque::new();
        
        // C1: Small candle
        candles.push_back(mock_candle(100.0, 101.0, 98.0, 99.0));
        // C2: Big impulsive candle
        candles.push_back(mock_candle(99.0, 99.0, 90.0, 91.0));
        // C3: Follow through
        candles.push_back(mock_candle(91.0, 95.0, 89.0, 90.0));

        // Low of C1 is 98.0, High of C3 is 95.0. Gap of 3.0.
        let fvg = strategy.detect_fvg(&candles);
        assert!(fvg.is_some());
        let (side, gap) = fvg.unwrap();
        assert_eq!(side, OrderSide::Sell);
        assert_eq!(gap, 3.0);
    }

    #[test]
    fn test_ob_detection() {
        let strategy = SMCStrategy::new(20, 0.001);
        let mut candles = VecDeque::new();
        
        // Add 5 candles
        candles.push_back(mock_candle(100.0, 101.0, 99.0, 100.5));
        candles.push_back(mock_candle(100.5, 102.0, 100.0, 101.5));
        candles.push_back(mock_candle(101.5, 103.0, 101.0, 102.0));
        // Bearish candle (Potential OB)
        candles.push_back(mock_candle(102.0, 102.5, 100.0, 100.5));
        // Followed by Bullish candle
        candles.push_back(mock_candle(101.0, 105.0, 101.0, 104.0));

        let ob = strategy.find_last_ob(&candles, OrderSide::Buy);
        assert!(ob.is_some());
        assert_eq!(ob.unwrap(), 100.0);
    }

    #[test]
    fn test_mss_detection() {
        let strategy = SMCStrategy::new(20, 0.001);
        let mut candles = VecDeque::new();
        
        // Add 9 candles with high around 110
        for _i in 0..9 {
            candles.push_back(mock_candle(100.0, 110.0, 90.0, 105.0));
        }
        // 10th candle breaks high
        candles.push_back(mock_candle(110.0, 115.0, 110.0, 114.0));

        let mss = strategy.detect_mss(&candles);
        assert_eq!(mss, Some(OrderSide::Buy));
    }
}
