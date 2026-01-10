use std::collections::HashMap;
use std::sync::RwLock;

/// Real-time spread data extracted from market quotes
#[derive(Debug, Clone)]
pub struct SpreadData {
    pub bid: f64,
    pub ask: f64,
    pub spread_bps: f64, // (ask - bid) / mid * 10000 basis points
    pub timestamp: i64,
}

/// Cache for storing real-time bid/ask spreads per symbol
pub struct SpreadCache {
    spreads: RwLock<HashMap<String, SpreadData>>,
}

// Manual Debug implementation for SpreadCache
impl std::fmt::Debug for SpreadCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpreadCache")
            .field("spreads", &"<RwLock>")
            .finish()
    }
}

impl SpreadCache {
    pub fn new() -> Self {
        Self {
            spreads: RwLock::new(HashMap::new()),
        }
    }

    /// Update spread data for a symbol from market quote
    pub fn update(&self, symbol: String, bid: f64, ask: f64) {
        let mid = (bid + ask) / 2.0;
        let spread_bps = if mid > 0.0 {
            ((ask - bid) / mid) * 10000.0 // Convert to basis points
        } else {
            0.0
        };

        // Log unusually high spreads for investigation
        if spread_bps > 50.0 {
            tracing::debug!(
                "SpreadCache: High spread detected for {} - bid={:.4}, ask={:.4}, spread={:.2} bps",
                symbol,
                bid,
                ask,
                spread_bps
            );
        }

        let data = SpreadData {
            bid,
            ask,
            spread_bps,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        match self.spreads.write() {
            Ok(mut guard) => {
                guard.insert(symbol, data);
            }
            Err(poisoned) => {
                tracing::error!("SpreadCache: Lock poisoned during write, recovering");
                poisoned.into_inner().insert(symbol, data);
            }
        }
    }

    /// Get spread as percentage (0.01 = 1%)
    pub fn get_spread_pct(&self, symbol: &str) -> Option<f64> {
        match self.spreads.read() {
            Ok(guard) => guard.get(symbol).map(|d| d.spread_bps / 10000.0),
            Err(poisoned) => poisoned
                .into_inner()
                .get(symbol)
                .map(|d| d.spread_bps / 10000.0),
        }
    }

    /// Get full spread data for a symbol
    pub fn get_spread_data(&self, symbol: &str) -> Option<SpreadData> {
        match self.spreads.read() {
            Ok(guard) => guard.get(symbol).cloned(),
            Err(poisoned) => poisoned.into_inner().get(symbol).cloned(),
        }
    }

    /// Check if spread data is stale (older than threshold_ms)
    pub fn is_stale(&self, symbol: &str, threshold_ms: i64) -> bool {
        let guard = match self.spreads.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(data) = guard.get(symbol) {
            let age_ms = chrono::Utc::now().timestamp_millis() - data.timestamp;
            age_ms > threshold_ms
        } else {
            true // No data = stale
        }
    }
}

impl Default for SpreadCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spread_calculation() {
        let cache = SpreadCache::new();

        // BTC example: bid=88700, ask=88710
        cache.update("BTC/USD".to_string(), 88700.0, 88710.0);

        let spread_pct = cache.get_spread_pct("BTC/USD").unwrap();

        // Expected: (88710 - 88700) / 88705 * 100 = 0.0112%
        assert!((spread_pct - 0.000112).abs() < 0.000001);
    }

    #[test]
    fn test_spread_data_storage() {
        let cache = SpreadCache::new();

        cache.update("ETH/USD".to_string(), 3000.0, 3001.0);

        let data = cache.get_spread_data("ETH/USD").unwrap();
        assert_eq!(data.bid, 3000.0);
        assert_eq!(data.ask, 3001.0);
    }

    #[test]
    fn test_stale_detection() {
        let cache = SpreadCache::new();

        cache.update("AVAX/USD".to_string(), 13.5, 13.52);

        // Fresh data
        assert!(!cache.is_stale("AVAX/USD", 60000)); // 60s threshold

        // Non-existent symbol is stale
        assert!(cache.is_stale("UNKNOWN", 60000));
    }
}
