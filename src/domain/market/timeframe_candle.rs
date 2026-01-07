use crate::domain::market::timeframe::Timeframe;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Represents an aggregated candle for a specific timeframe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeframeCandle {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: f64,
    /// Start timestamp of this timeframe period (in milliseconds)
    pub timestamp: i64,
    /// Number of 1-minute candles aggregated into this candle
    pub candle_count: usize,
}

impl TimeframeCandle {
    /// Creates a new TimeframeCandle from the first base candle
    pub fn new(
        symbol: String,
        timeframe: Timeframe,
        open: Decimal,
        high: Decimal,
        low: Decimal,
        close: Decimal,
        volume: f64,
        timestamp: i64,
    ) -> Self {
        Self {
            symbol,
            timeframe,
            open,
            high,
            low,
            close,
            volume,
            timestamp,
            candle_count: 1,
        }
    }

    /// Updates this candle with data from another candle (aggregation)
    /// 
    /// # Arguments
    /// * `candle` - The candle to merge into this one
    /// 
    /// # Note
    /// - Open remains unchanged (first candle's open)
    /// - High becomes max of all highs
    /// - Low becomes min of all lows
    /// - Close becomes the latest close
    /// - Volume is summed
    pub fn update(&mut self, _open: Decimal, high: Decimal, low: Decimal, close: Decimal, volume: f64) {
        // Open stays the same (first candle)
        // High is the maximum
        if high > self.high {
            self.high = high;
        }
        // Low is the minimum
        if low < self.low {
            self.low = low;
        }
        // Close is the latest
        self.close = close;
        // Volume is summed
        self.volume += volume;
        // Increment count
        self.candle_count += 1;
    }

    /// Checks if this candle is complete (has received all expected sub-candles)
    /// 
    /// For example, a 5-minute candle should have 5 one-minute candles
    pub fn is_complete(&self) -> bool {
        self.candle_count >= self.timeframe.to_minutes()
    }

    /// Returns the end timestamp of this timeframe period
    pub fn end_timestamp(&self) -> i64 {
        self.timestamp + (self.timeframe.to_seconds() * 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_new_timeframe_candle() {
        let candle = TimeframeCandle::new(
            "BTC/USD".to_string(),
            Timeframe::FiveMin,
            dec!(100.0),
            dec!(105.0),
            dec!(99.0),
            dec!(103.0),
            1000.0,
            1704067200000,
        );

        assert_eq!(candle.symbol, "BTC/USD");
        assert_eq!(candle.timeframe, Timeframe::FiveMin);
        assert_eq!(candle.open, dec!(100.0));
        assert_eq!(candle.close, dec!(103.0));
        assert_eq!(candle.candle_count, 1);
    }

    #[test]
    fn test_update_candle() {
        let mut candle = TimeframeCandle::new(
            "BTC/USD".to_string(),
            Timeframe::FiveMin,
            dec!(100.0),
            dec!(105.0),
            dec!(99.0),
            dec!(103.0),
            1000.0,
            1704067200000,
        );

        // Update with second minute
        candle.update(dec!(103.0), dec!(107.0), dec!(102.0), dec!(106.0), 1500.0);

        assert_eq!(candle.open, dec!(100.0)); // Unchanged
        assert_eq!(candle.high, dec!(107.0)); // Updated to max
        assert_eq!(candle.low, dec!(99.0)); // Unchanged (still min)
        assert_eq!(candle.close, dec!(106.0)); // Updated to latest
        assert_eq!(candle.volume, 2500.0); // Summed
        assert_eq!(candle.candle_count, 2);
    }

    #[test]
    fn test_is_complete() {
        let mut candle = TimeframeCandle::new(
            "BTC/USD".to_string(),
            Timeframe::FiveMin,
            dec!(100.0),
            dec!(105.0),
            dec!(99.0),
            dec!(103.0),
            1000.0,
            1704067200000,
        );

        assert!(!candle.is_complete()); // Only 1 of 5 candles

        for _ in 0..4 {
            candle.update(dec!(103.0), dec!(105.0), dec!(102.0), dec!(104.0), 1000.0);
        }

        assert!(candle.is_complete()); // All 5 candles received
    }

    #[test]
    fn test_end_timestamp() {
        let candle = TimeframeCandle::new(
            "BTC/USD".to_string(),
            Timeframe::FiveMin,
            dec!(100.0),
            dec!(105.0),
            dec!(99.0),
            dec!(103.0),
            1000.0,
            1704067200000, // 2024-01-01 00:00:00
        );

        // 5 minutes = 300 seconds = 300,000 ms
        assert_eq!(candle.end_timestamp(), 1704067200000 + 300_000);
    }
}
