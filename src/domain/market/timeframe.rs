use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents different timeframe intervals for market data analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timeframe {
    OneMin,
    FiveMin,
    FifteenMin,
    OneHour,
    FourHour,
    OneDay,
}

impl Timeframe {
    /// Returns the duration of this timeframe in minutes
    pub fn to_minutes(&self) -> usize {
        match self {
            Timeframe::OneMin => 1,
            Timeframe::FiveMin => 5,
            Timeframe::FifteenMin => 15,
            Timeframe::OneHour => 60,
            Timeframe::FourHour => 240,
            Timeframe::OneDay => 1440,
        }
    }

    /// Returns the duration in seconds
    pub fn to_seconds(&self) -> i64 {
        (self.to_minutes() * 60) as i64
    }

    /// Converts to Alpaca API timeframe string
    pub fn to_alpaca_string(&self) -> &'static str {
        match self {
            Timeframe::OneMin => "1Min",
            Timeframe::FiveMin => "5Min",
            Timeframe::FifteenMin => "15Min",
            Timeframe::OneHour => "1Hour",
            Timeframe::FourHour => "4Hour",
            Timeframe::OneDay => "1Day",
        }
    }

    /// Converts to Binance API interval string
    pub fn to_binance_string(&self) -> &'static str {
        match self {
            Timeframe::OneMin => "1m",
            Timeframe::FiveMin => "5m",
            Timeframe::FifteenMin => "15m",
            Timeframe::OneHour => "1h",
            Timeframe::FourHour => "4h",
            Timeframe::OneDay => "1d",
        }
    }

    /// Converts to OANDA API granularity string
    pub fn to_oanda_string(&self) -> &'static str {
        match self {
            Timeframe::OneMin => "M1",
            Timeframe::FiveMin => "M5",
            Timeframe::FifteenMin => "M15",
            Timeframe::OneHour => "H1",
            Timeframe::FourHour => "H4",
            Timeframe::OneDay => "D",
        }
    }

    /// Returns all available timeframes in ascending order
    pub fn all() -> Vec<Timeframe> {
        vec![
            Timeframe::OneMin,
            Timeframe::FiveMin,
            Timeframe::FifteenMin,
            Timeframe::OneHour,
            Timeframe::FourHour,
            Timeframe::OneDay,
        ]
    }

    /// Checks if a timestamp aligns with the start of this timeframe period
    ///
    /// # Arguments
    /// * `timestamp_ms` - Unix timestamp in milliseconds
    ///
    /// # Returns
    /// `true` if this timestamp represents the start of a new period for this timeframe
    pub fn is_period_start(&self, timestamp_ms: i64) -> bool {
        let timestamp_sec = timestamp_ms / 1000;
        let period_sec = self.to_seconds();

        match self {
            Timeframe::OneDay => {
                // Daily candles start at midnight UTC
                let seconds_since_midnight = timestamp_sec % 86400;
                seconds_since_midnight == 0
            }
            _ => {
                // Other timeframes: check if timestamp is divisible by period
                timestamp_sec % period_sec == 0
            }
        }
    }

    /// Returns the start timestamp of the period containing the given timestamp
    ///
    /// # Arguments
    /// * `timestamp_ms` - Unix timestamp in milliseconds
    ///
    /// # Returns
    /// The start timestamp (in ms) of the period containing this timestamp
    pub fn period_start(&self, timestamp_ms: i64) -> i64 {
        let timestamp_sec = timestamp_ms / 1000;
        let period_sec = self.to_seconds();

        let period_start_sec = match self {
            Timeframe::OneDay => {
                // Round down to midnight UTC
                timestamp_sec - (timestamp_sec % 86400)
            }
            _ => {
                // Round down to nearest period boundary
                timestamp_sec - (timestamp_sec % period_sec)
            }
        };

        period_start_sec * 1000
    }

    /// Calculates how many candles of this timeframe are needed for warmup
    ///
    /// # Arguments
    /// * `indicator_period` - The period of the indicator (e.g., 50 for SMA-50)
    ///
    /// # Returns
    /// Number of 1-minute candles needed to generate enough data for this timeframe
    pub fn warmup_candles(&self, indicator_period: usize) -> usize {
        // Need indicator_period candles of THIS timeframe
        // Each candle of this timeframe requires to_minutes() 1-min candles
        // Add 10% buffer
        let required = indicator_period * self.to_minutes();
        (required as f64 * 1.1) as usize
    }
}

impl FromStr for Timeframe {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "1m" | "1min" | "onemin" => Ok(Timeframe::OneMin),
            "5m" | "5min" | "fivemin" => Ok(Timeframe::FiveMin),
            "15m" | "15min" | "fifteenmin" => Ok(Timeframe::FifteenMin),
            "1h" | "1hour" | "onehour" => Ok(Timeframe::OneHour),
            "4h" | "4hour" | "fourhour" => Ok(Timeframe::FourHour),
            "1d" | "1day" | "oneday" => Ok(Timeframe::OneDay),
            _ => Err(anyhow!(
                "Invalid timeframe: '{}'. Valid options: 1Min, 5Min, 15Min, 1Hour, 4Hour, 1Day",
                s
            )),
        }
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_alpaca_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_minutes() {
        assert_eq!(Timeframe::OneMin.to_minutes(), 1);
        assert_eq!(Timeframe::FiveMin.to_minutes(), 5);
        assert_eq!(Timeframe::FifteenMin.to_minutes(), 15);
        assert_eq!(Timeframe::OneHour.to_minutes(), 60);
        assert_eq!(Timeframe::FourHour.to_minutes(), 240);
        assert_eq!(Timeframe::OneDay.to_minutes(), 1440);
    }

    #[test]
    fn test_from_str() {
        assert_eq!(Timeframe::from_str("1m").unwrap(), Timeframe::OneMin);
        assert_eq!(Timeframe::from_str("1Min").unwrap(), Timeframe::OneMin);
        assert_eq!(Timeframe::from_str("5m").unwrap(), Timeframe::FiveMin);
        assert_eq!(Timeframe::from_str("1h").unwrap(), Timeframe::OneHour);
        assert_eq!(Timeframe::from_str("4Hour").unwrap(), Timeframe::FourHour);
        assert_eq!(Timeframe::from_str("1d").unwrap(), Timeframe::OneDay);
        assert!(Timeframe::from_str("invalid").is_err());
    }

    #[test]
    fn test_period_start() {
        // Test 5-minute alignment
        let tf = Timeframe::FiveMin;
        // 2024-01-01 00:00:00 UTC = 1704067200000 ms
        let base = 1704067200000i64;

        // 00:00:00 should align to 00:00:00
        assert_eq!(tf.period_start(base), base);

        // 00:03:00 should align to 00:00:00
        assert_eq!(tf.period_start(base + 3 * 60 * 1000), base);

        // 00:05:00 should align to 00:05:00
        assert_eq!(tf.period_start(base + 5 * 60 * 1000), base + 5 * 60 * 1000);

        // 00:07:00 should align to 00:05:00
        assert_eq!(tf.period_start(base + 7 * 60 * 1000), base + 5 * 60 * 1000);
    }

    #[test]
    fn test_is_period_start() {
        let tf = Timeframe::FiveMin;
        let base = 1704067200000i64; // 2024-01-01 00:00:00 UTC

        assert!(tf.is_period_start(base)); // 00:00:00
        assert!(tf.is_period_start(base + 5 * 60 * 1000)); // 00:05:00
        assert!(!tf.is_period_start(base + 3 * 60 * 1000)); // 00:03:00
    }

    #[test]
    fn test_warmup_candles() {
        // For SMA-50 on 15-min timeframe
        let tf = Timeframe::FifteenMin;
        let warmup = tf.warmup_candles(50);

        // Need 50 * 15 = 750 minutes of data
        // With 10% buffer = 825 candles
        assert_eq!(warmup, 825);
    }

    #[test]
    fn test_api_strings() {
        assert_eq!(Timeframe::OneMin.to_alpaca_string(), "1Min");
        assert_eq!(Timeframe::OneMin.to_binance_string(), "1m");
        assert_eq!(Timeframe::OneMin.to_oanda_string(), "M1");

        assert_eq!(Timeframe::FourHour.to_alpaca_string(), "4Hour");
        assert_eq!(Timeframe::FourHour.to_binance_string(), "4h");
        assert_eq!(Timeframe::FourHour.to_oanda_string(), "H4");
    }
}
