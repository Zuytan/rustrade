use crate::application::market_data::spread_cache::SpreadCache;
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::types::Candle;
use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Debug)]
struct CandleBuilder {
    symbol: String,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: Decimal,
    start_time: DateTime<Utc>,
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
        }
    }

    fn update(&mut self, price: Decimal, quantity: Decimal, _timestamp: DateTime<Utc>) {
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
            timestamp: self.start_time.timestamp(),
        }
    }
}

pub struct CandleAggregator {
    // Map Symbol -> Current partial candle
    builders: HashMap<String, CandleBuilder>,
    _timeframe: Duration, // e.g., 1 minute
    repository: Option<Arc<dyn CandleRepository>>,
    #[allow(dead_code)]
    spread_cache: Arc<SpreadCache>, // Store real-time bid/ask spreads
}

impl CandleAggregator {
    pub fn new(
        repository: Option<Arc<dyn CandleRepository>>,
        spread_cache: Arc<SpreadCache>,
    ) -> Self {
        Self {
            builders: HashMap::new(),
            _timeframe: Duration::minutes(1),
            repository,
            spread_cache,
        }
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
    fn test_candle_aggregation() {
        let mut agg = CandleAggregator::new(None, Arc::new(SpreadCache::new()));
        let symbol = "BTC/USD";

        // T0: 00:00:01 - First tick, vol 1.5
        let t1 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 1)
            .unwrap()
            .timestamp_millis();
        let c1 = agg.on_quote(symbol, dec!(100), dec!(1.5), t1);
        assert!(c1.is_none());

        // T1: 00:00:30 - Update, vol 2.5
        let t2 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 30)
            .unwrap()
            .timestamp_millis();
        let c2 = agg.on_quote(symbol, dec!(105), dec!(2.5), t2);
        assert!(c2.is_none()); // Still same minute

        // T2: 00:00:59 - Low, vol 1.0
        let t3 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 59)
            .unwrap()
            .timestamp_millis();
        let c3 = agg.on_quote(symbol, dec!(95), dec!(1.0), t3);
        assert!(c3.is_none());

        // T3: 00:01:05 - NEW MINUTE -> Trigger close of previous. Vol 0.5 (goes to next candle)
        let t4 = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 1, 5)
            .unwrap()
            .timestamp_millis();
        let c4 = agg.on_quote(symbol, dec!(100), dec!(0.5), t4);

        assert!(c4.is_some());
        let candle = c4.unwrap();
        assert_eq!(candle.open, dec!(100));
        assert_eq!(candle.high, dec!(105));
        assert_eq!(candle.low, dec!(95));
        assert_eq!(candle.close, dec!(95)); // Last tick of minute 0
        assert_eq!(candle.volume, dec!(5.0)); // 1.5 + 2.5 + 1.0 = 5.0
        assert_eq!(
            candle.timestamp,
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
    }
}
