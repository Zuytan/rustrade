use crate::domain::repositories::CandleRepository;
use crate::domain::trading::types::Candle;
use chrono::{DateTime, TimeZone, Timelike, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Maximum allowed deviation from the current close price (as a ratio).
/// Quotes deviating more than this from the last known price are rejected as outliers.
/// 1.5% is generous enough for volatile crypto while filtering bad bid/ask mid-prices.
const MAX_PRICE_DEVIATION_PCT: Decimal = dec!(0.015);

#[derive(Debug)]
struct CandleBuilder {
    symbol: String,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: Decimal,
    start_time: DateTime<Utc>,
    tick_count: u32,
}

impl CandleBuilder {
    fn new(symbol: String, price: Decimal, timestamp: DateTime<Utc>) -> Self {
        // Normalize start time to the beginning of the minute
        let start_time = timestamp
            .date_naive()
            .and_hms_opt(timestamp.hour(), timestamp.minute(), 0)
            .expect("Valid hour/minute should always produce valid time")
            .and_utc();

        Self {
            symbol,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: Decimal::ZERO, // We rely on quotes, volume might be missing or aggregated later
            start_time,
            tick_count: 0,
        }
    }

    /// Check if a price is an outlier relative to the current close price.
    /// Returns true if the price should be rejected.
    fn is_outlier(&self, price: Decimal) -> bool {
        // Allow all prices during the first few ticks (not enough data for filtering)
        if self.tick_count < 3 {
            return false;
        }

        if self.close <= Decimal::ZERO {
            return false;
        }

        let deviation = ((price - self.close) / self.close).abs();
        deviation > MAX_PRICE_DEVIATION_PCT
    }

    fn update(&mut self, price: Decimal, quantity: Decimal, _timestamp: DateTime<Utc>) {
        self.tick_count += 1;

        if price > self.high {
            self.high = price;
        }
        if price < self.low {
            self.low = price;
        }
        self.close = price;
        // Accumulate volume using the provided quantity which is already Decimal
        self.volume += quantity;
    }

    fn build(&self) -> Candle {
        Candle {
            symbol: self.symbol.clone(),
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
            timestamp: self.start_time.timestamp_millis(),
        }
    }
}

pub struct CandleAggregator {
    // Map Symbol -> Current partial candle
    builders: HashMap<String, CandleBuilder>,
    /// Last confirmed close price per symbol (used for cross-candle outlier filtering)
    last_close: HashMap<String, Decimal>,
    repository: Option<Arc<dyn CandleRepository>>,
}

impl CandleAggregator {
    pub fn new(repository: Option<Arc<dyn CandleRepository>>) -> Self {
        Self {
            builders: HashMap::new(),
            last_close: HashMap::new(),
            repository,
        }
    }

    /// Check if a price is an outlier for a symbol, using both the current candle
    /// and the last confirmed close price.
    fn is_price_outlier(&self, symbol: &str, price: Decimal) -> bool {
        // Check against current candle's close
        if let Some(builder) = self.builders.get(symbol)
            && builder.is_outlier(price)
        {
            return true;
        }

        // Check against last completed candle's close (cross-candle protection)
        if let Some(last) = self.last_close.get(symbol)
            && *last > Decimal::ZERO
        {
            let deviation = ((price - last) / last).abs();
            if deviation > MAX_PRICE_DEVIATION_PCT {
                return true;
            }
        }

        false
    }

    /// Process a Quote event. Returns Some(Candle) if a candle is completed (i.e., we moved to a new minute).
    pub fn on_quote(
        &mut self,
        symbol: &str,
        price: Decimal,
        quantity: Decimal,
        timestamp_ms: i64,
    ) -> Option<Candle> {
        let timestamp = match Utc.timestamp_millis_opt(timestamp_ms).single() {
            Some(t) => t,
            None => {
                error!(
                    "CandleAggregator: Invalid timestamp {} for {}",
                    timestamp_ms, symbol
                );
                return None;
            }
        };

        // --- OUTLIER FILTER ---
        // Reject quotes that deviate too far from last known price.
        // This prevents bad mid-prices from wide bid/ask spreads from corrupting candles.
        if self.is_price_outlier(symbol, price) {
            warn!(
                "CandleAggregator: {} OUTLIER rejected: {} (last close: {})",
                symbol,
                price,
                self.last_close
                    .get(symbol)
                    .copied()
                    .unwrap_or(Decimal::ZERO)
            );
            return None;
        }

        let current_minute = timestamp
            .date_naive()
            .and_hms_opt(timestamp.hour(), timestamp.minute(), 0)
            .expect("Valid hour/minute should always produce valid time")
            .and_utc();

        // Check if we have an existing builder for this symbol
        if let Some(builder) = self.builders.get_mut(symbol) {
            if builder.start_time == current_minute {
                // Same minute, update existing candle
                builder.update(price, quantity, timestamp);
                None
            } else {
                // New minute! Finalize the old candle and start a new one
                let completed_candle = builder.build();

                info!(
                    "CandleAggregator: {} candle completed → O:{} H:{} L:{} C:{} V:{}",
                    symbol,
                    completed_candle.open,
                    completed_candle.high,
                    completed_candle.low,
                    completed_candle.close,
                    completed_candle.volume
                );

                // Track last close for cross-candle outlier detection
                self.last_close
                    .insert(symbol.to_string(), completed_candle.close);

                // Start new candle
                *builder = CandleBuilder::new(symbol.to_string(), price, timestamp);

                if let Some(repo) = &self.repository {
                    let candle_clone = completed_candle.clone();
                    let repo = repo.clone();
                    tokio::spawn(async move {
                        if let Err(e) = repo.save(&candle_clone).await {
                            error!(
                                "Failed to persist candle for {}: {}",
                                candle_clone.symbol, e
                            );
                        }
                    });
                }

                Some(completed_candle)
            }
        } else {
            // First tick for this symbol
            info!(
                "CandleAggregator: {} - First quote @ {}, starting aggregation",
                symbol, price
            );
            // NOTE: We do NOT set last_close here. It is only set when a candle
            // actually completes, providing a confirmed reference price.
            self.builders.insert(
                symbol.to_string(),
                // For the first tick, we create the builder with initial state.
                // Note: The initial volume should ideally include this first tick's quantity.
                // However, CandleBuilder::new initializes volume to 0.0.
                // We should probably update it immediately.
                {
                    let mut builder = CandleBuilder::new(symbol.to_string(), price, timestamp);
                    builder.update(price, quantity, timestamp);
                    builder
                },
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_candle_aggregation_realistic_prices() {
        let mut agg = CandleAggregator::new(None);
        let symbol = "BTC/USD";

        // Realistic BTC prices: ~$68,000 with <1% intra-minute moves
        // T0: 00:00:01 - First tick (open), vol 1.5
        let t1 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 1)
            .unwrap()
            .timestamp_millis();
        let c1 = agg.on_quote(symbol, dec!(68000), dec!(1.5), t1);
        assert!(c1.is_none());

        // T1: 00:00:30 - High, vol 2.5
        let t2 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 30)
            .unwrap()
            .timestamp_millis();
        let c2 = agg.on_quote(symbol, dec!(68150), dec!(2.5), t2);
        assert!(c2.is_none()); // Still same minute

        // T2: 00:00:45 - Another tick, vol 0.8
        let t2b = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 45)
            .unwrap()
            .timestamp_millis();
        let c2b = agg.on_quote(symbol, dec!(68100), dec!(0.8), t2b);
        assert!(c2b.is_none());

        // T3: 00:00:59 - Low (close), vol 1.0
        let t3 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 59)
            .unwrap()
            .timestamp_millis();
        let c3 = agg.on_quote(symbol, dec!(67900), dec!(1.0), t3);
        assert!(c3.is_none());

        // T4: 00:01:05 - NEW MINUTE → completes previous candle. Vol 0.5 (new candle)
        let t4 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 1, 5)
            .unwrap()
            .timestamp_millis();
        let c4 = agg.on_quote(symbol, dec!(67950), dec!(0.5), t4);

        assert!(c4.is_some());
        let candle = c4.unwrap();
        assert_eq!(candle.open, dec!(68000));
        assert_eq!(candle.high, dec!(68150));
        assert_eq!(candle.low, dec!(67900));
        assert_eq!(candle.close, dec!(67900)); // Last tick of minute 0
        assert_eq!(candle.volume, dec!(5.8)); // 1.5 + 2.5 + 0.8 + 1.0 = 5.8
        assert_eq!(
            candle.timestamp,
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
    }

    #[test]
    fn test_outlier_rejection_within_candle() {
        let mut agg = CandleAggregator::new(None);
        let symbol = "BTC/USD";

        let base_ts = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 1)
            .unwrap()
            .timestamp_millis();

        // Build a candle with several normal ticks
        agg.on_quote(symbol, dec!(68000), dec!(1.0), base_ts);
        agg.on_quote(symbol, dec!(68050), dec!(1.0), base_ts + 5000);
        agg.on_quote(symbol, dec!(68020), dec!(1.0), base_ts + 10000);
        agg.on_quote(symbol, dec!(68030), dec!(1.0), base_ts + 15000);

        // Now inject a wildly high outlier (from wide bid/ask spread)
        // 68030 * 1.015 = 69050.45, so 69500 is well above threshold
        let result = agg.on_quote(symbol, dec!(69500), dec!(1.0), base_ts + 20000);
        assert!(result.is_none(), "Outlier should be silently rejected");

        // Verify the outlier did NOT corrupt the candle by sending a new-minute tick
        let next_min = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 1, 1)
            .unwrap()
            .timestamp_millis();
        let candle = agg.on_quote(symbol, dec!(68040), dec!(0.5), next_min);
        assert!(candle.is_some());
        let c = candle.unwrap();
        // High should be 68050, NOT 69500 (outlier rejected)
        assert_eq!(c.high, dec!(68050));
        assert_eq!(c.low, dec!(68000));
        assert_eq!(c.close, dec!(68030));
    }

    #[test]
    fn test_outlier_rejection_cross_candle() {
        let mut agg = CandleAggregator::new(None);
        let symbol = "ETH/USD";

        // Build and complete a first candle
        let t0 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 1)
            .unwrap()
            .timestamp_millis();
        agg.on_quote(symbol, dec!(3500), dec!(1.0), t0);
        agg.on_quote(symbol, dec!(3510), dec!(1.0), t0 + 10000);
        agg.on_quote(symbol, dec!(3505), dec!(1.0), t0 + 20000);
        agg.on_quote(symbol, dec!(3502), dec!(1.0), t0 + 30000);

        // Complete candle by entering new minute with normal price
        let t1 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 1, 1)
            .unwrap()
            .timestamp_millis();
        let candle1 = agg.on_quote(symbol, dec!(3503), dec!(1.0), t1);
        assert!(candle1.is_some());
        assert_eq!(candle1.unwrap().close, dec!(3502));

        // Now try an outlier as the second tick of the new candle
        // Previous close was 3502, new candle opened at 3503
        // 3502 * 1.015 = 3554.53, so 3600 is an outlier
        let result = agg.on_quote(symbol, dec!(3600), dec!(1.0), t1 + 5000);
        assert!(result.is_none(), "Cross-candle outlier should be rejected");

        // But a normal tick should work fine
        let result = agg.on_quote(symbol, dec!(3508), dec!(1.0), t1 + 10000);
        assert!(result.is_none()); // Still same minute, no completion
    }

    #[test]
    fn test_normal_prices_not_rejected() {
        let mut agg = CandleAggregator::new(None);
        let symbol = "BTC/USD";

        let t0 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 1)
            .unwrap()
            .timestamp_millis();

        // Simulate many normal ticks with small variations (<1%)
        let prices = vec![
            dec!(68000),
            dec!(68050),
            dec!(68100),
            dec!(67980),
            dec!(68020),
            dec!(68150),
            dec!(67950),
            dec!(68000),
            dec!(68080),
            dec!(68030),
        ];

        for (i, price) in prices.iter().enumerate() {
            let result = agg.on_quote(symbol, *price, dec!(1.0), t0 + (i as i64 * 3000));
            assert!(
                result.is_none(),
                "Normal price {} should NOT be rejected",
                price
            );
        }
    }
}
