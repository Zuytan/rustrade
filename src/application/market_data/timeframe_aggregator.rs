use crate::domain::market::timeframe::Timeframe;
use crate::domain::market::timeframe_candle::TimeframeCandle;
use crate::domain::trading::types::Candle;
use std::collections::HashMap;

/// Aggregates 1-minute candles into higher timeframes
///
/// This service maintains state for each symbol and timeframe combination,
/// accumulating 1-minute candles until a complete higher-timeframe candle is formed.
pub struct TimeframeAggregator {
    /// Active (incomplete) candles being built for each symbol and timeframe
    /// Key: (symbol, timeframe), Value: incomplete TimeframeCandle
    active_candles: HashMap<(String, Timeframe), TimeframeCandle>,
}

impl TimeframeAggregator {
    pub fn new() -> Self {
        Self {
            active_candles: HashMap::new(),
        }
    }

    /// Process a 1-minute candle and generate higher timeframe candles if periods complete
    ///
    /// # Arguments
    /// * `candle` - The 1-minute base candle to process
    /// * `timeframes` - The timeframes to aggregate into
    ///
    /// # Returns
    /// A vector of completed TimeframeCandles (may be empty if no periods completed)
    pub fn process_candle(
        &mut self,
        candle: &Candle,
        timeframes: &[Timeframe],
    ) -> Vec<TimeframeCandle> {
        let mut completed_candles = Vec::new();

        for &timeframe in timeframes {
            // Skip 1-minute timeframe (no aggregation needed)
            if timeframe == Timeframe::OneMin {
                continue;
            }

            let key = (candle.symbol.clone(), timeframe);
            let period_start = timeframe.period_start(candle.timestamp);

            // Check if we have an active candle for this period
            if let Some(active) = self.active_candles.get_mut(&key) {
                // Check if this candle belongs to the current period
                if active.timestamp == period_start {
                    // Update existing candle
                    active.update(
                        candle.open,
                        candle.high,
                        candle.low,
                        candle.close,
                        candle.volume,
                    );

                    // Check if candle is complete
                    if active.is_complete() {
                        // Move completed candle to output
                        let completed = self
                            .active_candles
                            .remove(&key)
                            .expect("active_candle verified to exist by get_mut check");
                        completed_candles.push(completed);
                    }
                } else {
                    // New period started - complete the old one and start a new one
                    let completed = self
                        .active_candles
                        .remove(&key)
                        .expect("active_candle verified to exist by get_mut check");
                    completed_candles.push(completed);

                    // Start new candle for new period
                    let new_candle = TimeframeCandle::new(
                        candle.symbol.clone(),
                        timeframe,
                        candle.open,
                        candle.high,
                        candle.low,
                        candle.close,
                        candle.volume,
                        period_start,
                    );
                    self.active_candles.insert(key, new_candle);
                }
            } else {
                // No active candle - start a new one
                let new_candle = TimeframeCandle::new(
                    candle.symbol.clone(),
                    timeframe,
                    candle.open,
                    candle.high,
                    candle.low,
                    candle.close,
                    candle.volume,
                    period_start,
                );
                self.active_candles.insert(key, new_candle);
            }
        }

        completed_candles
    }

    /// Manually complete all active candles (useful for end-of-session or testing)
    ///
    /// # Arguments
    /// * `symbol` - Optional symbol filter (if None, completes all symbols)
    ///
    /// # Returns
    /// All active candles that were completed
    pub fn flush(&mut self, symbol: Option<&str>) -> Vec<TimeframeCandle> {
        if let Some(sym) = symbol {
            // Flush only for specific symbol
            let keys_to_remove: Vec<_> = self
                .active_candles
                .keys()
                .filter(|(s, _)| s == sym)
                .cloned()
                .collect();

            keys_to_remove
                .into_iter()
                .filter_map(|key| self.active_candles.remove(&key))
                .collect()
        } else {
            // Flush all
            self.active_candles.drain().map(|(_, v)| v).collect()
        }
    }

    /// Get the current state of an active candle (for debugging/monitoring)
    pub fn get_active_candle(
        &self,
        symbol: &str,
        timeframe: Timeframe,
    ) -> Option<&TimeframeCandle> {
        self.active_candles.get(&(symbol.to_string(), timeframe))
    }

    /// Clear all state (useful for testing)
    pub fn clear(&mut self) {
        self.active_candles.clear();
    }
}

impl Default for TimeframeAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn create_test_candle(symbol: &str, timestamp: i64, close: f64) -> Candle {
        Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(close).unwrap(),
            high: Decimal::from_f64_retain(close + 1.0).unwrap(),
            low: Decimal::from_f64_retain(close - 1.0).unwrap(),
            close: Decimal::from_f64_retain(close).unwrap(),
            volume: 1000.0,
            timestamp,
        }
    }

    #[test]
    fn test_aggregate_5min_candles() {
        let mut aggregator = TimeframeAggregator::new();
        let timeframes = vec![Timeframe::FiveMin];

        // Base timestamp: 2024-01-01 00:00:00 UTC = 1704067200000 ms
        let base = 1704067200000i64;

        // Send 5 one-minute candles
        for i in 0..5 {
            let timestamp = base + (i * 60 * 1000);
            let candle = create_test_candle("BTC/USD", timestamp, 100.0 + i as f64);
            let completed = aggregator.process_candle(&candle, &timeframes);

            if i < 4 {
                // First 4 candles should not complete the 5-min period
                assert_eq!(completed.len(), 0);
            } else {
                // 5th candle completes the period
                assert_eq!(completed.len(), 1);
                let tf_candle = &completed[0];
                assert_eq!(tf_candle.timeframe, Timeframe::FiveMin);
                assert_eq!(tf_candle.open, dec!(100.0)); // First candle's open
                assert_eq!(tf_candle.close, dec!(104.0)); // Last candle's close
                assert_eq!(tf_candle.candle_count, 5);
            }
        }
    }

    #[test]
    fn test_multiple_timeframes() {
        let mut aggregator = TimeframeAggregator::new();
        let timeframes = vec![Timeframe::FiveMin, Timeframe::FifteenMin];

        let base = 1704067200000i64;

        // Send 15 one-minute candles
        let mut completed_5min = 0;
        let mut completed_15min = 0;

        for i in 0..15 {
            let timestamp = base + (i * 60 * 1000);
            let candle = create_test_candle("BTC/USD", timestamp, 100.0);
            let completed = aggregator.process_candle(&candle, &timeframes);

            for tf_candle in completed {
                match tf_candle.timeframe {
                    Timeframe::FiveMin => completed_5min += 1,
                    Timeframe::FifteenMin => completed_15min += 1,
                    _ => {}
                }
            }
        }

        // Should have 3 complete 5-min candles (0-4, 5-9, 10-14)
        assert_eq!(completed_5min, 3);
        // Should have 1 complete 15-min candle (0-14)
        assert_eq!(completed_15min, 1);
    }

    #[test]
    fn test_period_boundary() {
        let mut aggregator = TimeframeAggregator::new();
        let timeframes = vec![Timeframe::FiveMin];

        let base = 1704067200000i64;

        // Send 4 candles (incomplete period)
        for i in 0..4 {
            let timestamp = base + (i * 60 * 1000);
            let candle = create_test_candle("BTC/USD", timestamp, 100.0);
            aggregator.process_candle(&candle, &timeframes);
        }

        // Jump to next period (minute 5 -> minute 10)
        let candle = create_test_candle("BTC/USD", base + (10 * 60 * 1000), 100.0);
        let completed = aggregator.process_candle(&candle, &timeframes);

        // Should complete the first period even though it only had 4 candles
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].candle_count, 4);
    }

    #[test]
    fn test_flush() {
        let mut aggregator = TimeframeAggregator::new();
        let timeframes = vec![Timeframe::FiveMin];

        let base = 1704067200000i64;

        // Send 3 candles (incomplete period)
        for i in 0..3 {
            let timestamp = base + (i * 60 * 1000);
            let candle = create_test_candle("BTC/USD", timestamp, 100.0);
            aggregator.process_candle(&candle, &timeframes);
        }

        // Flush should return the incomplete candle
        let flushed = aggregator.flush(Some("BTC/USD"));
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].candle_count, 3);
        assert!(!flushed[0].is_complete());
    }
}
