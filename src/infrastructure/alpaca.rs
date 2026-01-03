use crate::config::AssetClass; // Added
use crate::domain::ports::OrderUpdate; // Added
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::trading::types::{normalize_crypto_symbol, MarketEvent, Order, OrderSide};
use crate::infrastructure::alpaca_trading_stream::AlpacaTradingStream; // Added
use crate::infrastructure::alpaca_websocket::AlpacaWebSocketManager;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::NaiveDate;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{
    broadcast,
    mpsc::{self, Receiver},
}; // Added broadcast
use tracing::{debug, error, info, trace}; // Restored imports

// ===== Constants =====

/// Major crypto pairs to scan for top movers
/// Since Alpaca doesn't provide a movers API for crypto, we maintain a curated list
const CRYPTO_UNIVERSE: &[&str] = &[
    "BTC/USD",
    "ETH/USD",
    "AVAX/USD",
    "SOL/USD",
    "MATIC/USD",
    "LINK/USD",
    "UNI/USD",
    "AAVE/USD",
    "DOT/USD",
    "ATOM/USD",
];

// ===== Market Data Service (WebSocket) =====

pub struct AlpacaMarketDataService {
    client: Client,
    api_key: String,
    api_secret: String,
    ws_manager: Arc<AlpacaWebSocketManager>, // Singleton WebSocket manager
    data_base_url: String,
    bar_cache: std::sync::RwLock<std::collections::HashMap<String, Vec<AlpacaBar>>>,
    min_volume_threshold: f64,
    asset_class: AssetClass, // Added
    spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>, // Shared spread cache for real-time cost tracking
}

impl AlpacaMarketDataService {
    pub fn new(
        api_key: String,
        api_secret: String,
        ws_url: String,
        data_base_url: String,
        min_volume_threshold: f64,
        asset_class: AssetClass, // Added
    ) -> Self {
        // Configure client with connection pool limits
        let client = Client::builder()
            .pool_max_idle_per_host(5)
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        // Create singleton WebSocket manager
        let spread_cache =
            Arc::new(crate::application::market_data::spread_cache::SpreadCache::new());
        let ws_manager = Arc::new(AlpacaWebSocketManager::new(
            api_key.clone(),
            api_secret.clone(),
            ws_url,
            spread_cache.clone(), // Clone for WebSocket manager
        ));

        Self {
            client,
            api_key,
            api_secret,
            ws_manager,
            data_base_url,
            bar_cache: std::sync::RwLock::new(std::collections::HashMap::new()),
            min_volume_threshold,
            asset_class,
            spread_cache, // Store for external access
        }
    }

    /// Get the shared spread cache for real-time bid/ask tracking
    /// This should be shared with CostEvaluator to use real spreads instead of defaults
    pub fn get_spread_cache(&self) -> Arc<crate::application::market_data::spread_cache::SpreadCache> {
        self.spread_cache.clone()
    }
}

#[async_trait]
impl MarketDataService for AlpacaMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        // Update subscription on the singleton WebSocket manager
        self.ws_manager.update_subscription(symbols).await?;

        // Get a broadcast receiver from the manager
        let mut broadcast_rx = self.ws_manager.subscribe();

        // Convert broadcast to mpsc for API compatibility
        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            // Receiver dropped, exit
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // Receiver fell behind, log and continue
                        // This is common during high-activity periods
                        tracing::warn!(
                            "Market event broadcast receiver lagged, missed {} messages",
                            n
                        );
                        // Continue receiving - don't break the loop
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Channel closed, exit gracefully
                        tracing::debug!("Market event broadcast channel closed");
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        if self.asset_class == AssetClass::Crypto {
            info!("MarketScanner: Scanning crypto top movers from universe of {} pairs", CRYPTO_UNIVERSE.len());
            return self.get_crypto_top_movers().await;
        }

        // Alpaca Data v1beta1 Screener Movers endpoint
        let url = format!("{}/v1beta1/screener/stocks/movers", self.data_base_url);

        let mut response = self
            .client
            .get(url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to fetch top movers from Alpaca (v1beta1)")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            info!(
                "Alpaca v1beta1 movers failed: {}. Falling back to v2/stocks/movers...",
                error_text
            );

            // Fallback to V2 movers endpoint
            let v2_url = format!("{}/v2/stocks/movers", self.data_base_url);
            response = self
                .client
                .get(v2_url)
                .header("APCA-API-KEY-ID", &self.api_key)
                .header("APCA-API-SECRET-KEY", &self.api_secret)
                .send()
                .await
                .context("Failed to fetch top movers from Alpaca (v2 fallback)")?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                anyhow::bail!(
                    "Alpaca movers fetch failed (both v1beta1 and v2): {}",
                    error_text
                );
            }
        }

        #[derive(Debug, Deserialize)]
        struct Mover {
            symbol: String,
            #[serde(default)]
            #[allow(dead_code)]
            price: f64,
        }

        let json_val: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse movers JSON")?;

        let movers: Vec<Mover> = if let Some(gainers) = json_val.get("gainers") {
            if gainers.is_null() {
                vec![]
            } else {
                serde_json::from_value(gainers.clone())?
            }
        } else if let Some(movers) = json_val.as_array() {
            serde_json::from_value(serde_json::Value::Array(movers.clone()))?
        } else {
            vec![]
        };

        if movers.is_empty() {
            info!("MarketScanner: No movers found in Alpaca response.");
            return Ok(vec![]);
        }

        // 1. Initial Filter (Structure)
        let candidates: Vec<String> = movers
            .into_iter()
            .filter(|m| {
                let is_warrant = m.symbol.contains(".WS") || m.symbol.ends_with('W');
                let is_unit = m.symbol.ends_with('U');
                !is_warrant && !is_unit
            })
            .map(|m| m.symbol)
            .collect();

        if candidates.is_empty() {
            return Ok(vec![]);
        }

        info!(
            "MarketScanner: Validating {} candidates via Snapshots...",
            candidates.len()
        );

        // 2. Fetch Snapshots for Volume Verification
        // Reuse get_prices logic but we need access to Snapshot struct which was internal.
        // We will just call the raw API here to avoid exposing internal logic or modifying get_prices return type too much.
        let url = format!("{}/v2/stocks/snapshots", self.data_base_url);
        let symbols_param = candidates.join(",");

        let response = self
            .client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[("symbols", &symbols_param)])
            .send()
            .await
            .context("Failed to fetch snapshots for validation")?;

        if !response.status().is_success() {
            // If snapshot fails, fallback to just returning candidates but warn
            let err = response.text().await.unwrap_or_default();
            error!(
                "MarketScanner: Snapshot validation failed: {}. Returning raw candidates.",
                err
            );
            return Ok(candidates);
        }

        #[derive(Debug, Deserialize)]
        struct SnapshotTrade {
            #[serde(rename = "p")]
            price: f64,
        }
        #[derive(Debug, Deserialize)]
        struct SnapshotDay {
            #[serde(rename = "v")]
            volume: f64,
        }
        #[derive(Debug, Deserialize)]
        struct Snapshot {
            #[serde(rename = "latestTrade")]
            latest_trade: Option<SnapshotTrade>,
            #[serde(rename = "dailyBar")]
            daily_bar: Option<SnapshotDay>,
            #[serde(rename = "prevDailyBar")]
            prev_daily_bar: Option<SnapshotDay>, // Fallback for volume?
        }

        let snapshots: std::collections::HashMap<String, Snapshot> = response.json().await?;

        let filtered_symbols: Vec<String> = candidates
            .into_iter()
            .filter(|sym| {
                if let Some(snap) = snapshots.get(sym) {
                    let price = snap.latest_trade.as_ref().map(|t| t.price).unwrap_or(0.0);

                    // Check Volume: dailyBar or prevDailyBar
                    let volume = snap
                        .daily_bar
                        .as_ref()
                        .map(|b| b.volume)
                        .or_else(|| snap.prev_daily_bar.as_ref().map(|b| b.volume))
                        .unwrap_or(0.0);

                    let is_penny = price < 5.0;
                    let has_volume = volume >= self.min_volume_threshold;

                    if !has_volume {
                        // Debug log for exclusion
                        // info!("Excluded {} - Volume: {} < {}", sym, volume, self.min_volume_threshold);
                    }

                    price > 0.0 && !is_penny && has_volume
                } else {
                    false
                }
            })
            .collect();

        info!(
            "MarketScanner: Final filtered movers: {} (from {})",
            filtered_symbols.len(),
            snapshots.len()
        );
        Ok(filtered_symbols)
    }

    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, rust_decimal::Decimal>> {
        if symbols.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Detect if we're dealing with crypto symbols (contain '/')
        let is_crypto = symbols.iter().any(|s| s.contains('/'));
        
        // For crypto, denormalize symbols (remove slashes) before API call
        // Alpaca's snapshot API expects BTCUSD not BTC/USD
        let api_symbols: Vec<String> = if is_crypto {
            symbols.iter().map(|s| crate::domain::trading::types::denormalize_crypto_symbol(s)).collect()
        } else {
            symbols.clone()
        };

        let url = format!("{}/v2/stocks/snapshots", self.data_base_url);
        let symbols_param = api_symbols.join(",");

        let response = self
            .client
            .get(url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[("symbols", &symbols_param)])
            .send()
            .await
            .context("Failed to fetch snapshots from Alpaca")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca snapshots fetch failed: {}", error_text);
        }

        // Response structure: Keys are symbols, Values are Snapshot objects
        // Snapshot object has "dailyBar" or "latestTrade" or "latestQuote".
        // We prefer "latestTrade" price.
        #[derive(Debug, Deserialize)]
        struct SnapshotTrade {
            #[serde(rename = "p")]
            price: f64,
        }
        #[derive(Debug, Deserialize)]
        struct Snapshot {
            #[serde(rename = "latestTrade")]
            latest_trade: Option<SnapshotTrade>,
            #[serde(rename = "prevDailyBar")]
            prev_daily_bar: Option<AlpacaBar>, // Fallback
        }

        // Alpaca returns a map of symbol -> snapshot
        let resp: std::collections::HashMap<String, Snapshot> = response
            .json()
            .await
            .context("Failed to parse Alpaca snapshots response")?;

        let mut prices = std::collections::HashMap::new();

        for (alp_sym, snapshot) in resp {
            // Normalize the symbol back to internal format (BTCUSD -> BTC/USD)
            let normalized_sym = if is_crypto {
                crate::domain::trading::types::normalize_crypto_symbol(&alp_sym)
                    .unwrap_or(alp_sym.clone())
            } else {
                alp_sym
            };
            
            let price_f64 = if let Some(trade) = snapshot.latest_trade {
                trade.price
            } else if let Some(bar) = snapshot.prev_daily_bar {
                bar.close
            } else {
                0.0
            };

            if price_f64 > 0.0 {
                if let Some(dec) = rust_decimal::Decimal::from_f64_retain(price_f64) {
                    prices.insert(normalized_sym, dec);
                }
            }
        }

        Ok(prices)
    }

    async fn get_historical_bars(
        &self,
        symbol: &str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        timeframe: &str,
    ) -> Result<Vec<crate::domain::trading::types::Candle>> {
        // Use the internal implementation
        let alpaca_bars = self
            .fetch_historical_bars_internal(symbol, start, end, timeframe)
            .await?;

        // Convert AlpacaBar -> Candle
        let candles = alpaca_bars
            .into_iter()
            .map(|b| {
                let timestamp = chrono::DateTime::parse_from_rfc3339(&b.timestamp)
                    .unwrap_or_default()
                    .timestamp();

                crate::domain::trading::types::Candle {
                    symbol: symbol.to_string(),
                    open: Decimal::from_f64_retain(b.open).unwrap_or(Decimal::ZERO),
                    high: Decimal::from_f64_retain(b.high).unwrap_or(Decimal::ZERO),
                    low: Decimal::from_f64_retain(b.low).unwrap_or(Decimal::ZERO),
                    close: Decimal::from_f64_retain(b.close).unwrap_or(Decimal::ZERO),
                    volume: b.volume,
                    timestamp,
                }
            })
            .collect();

        Ok(candles)
    }
}

impl AlpacaMarketDataService {
    async fn fetch_historical_bars_internal(
        &self,
        symbol: &str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        timeframe: &str,
    ) -> Result<Vec<AlpacaBar>> {
        // 1. Check Cache
        let cache_key = format!(
            "{}:{}:{}:{}",
            symbol,
            start.timestamp(),
            end.timestamp(),
            timeframe
        );

        {
            let cache = self.bar_cache.read().unwrap();
            if let Some(bars) = cache.get(&cache_key) {
                trace!("AlpacaMarketDataService: Cache HIT for {}", cache_key);
                return Ok(bars.clone());
            }
        }

        debug!(
            "AlpacaMarketDataService: Cache MISS for {}. Fetching...",
            cache_key
        );

        // Determine endpoint based on symbol format (Crypto pairs usually have '/')
        let is_crypto = symbol.contains('/');
        let url = if is_crypto {
            // For crypto, use v1beta3 endpoint
            format!("{}/v1beta3/crypto/us/bars", self.data_base_url)
        } else {
            format!("{}/v2/stocks/bars", self.data_base_url)
        };

        let mut all_bars = Vec::new();
        let mut page_token: Option<String> = None;
        let mut _backoff = 1; // Unused but kept for structure

        loop {
            let mut query_params = vec![
                ("symbols", symbol.to_string()),
                ("start", start.to_rfc3339()),
                ("end", end.to_rfc3339()),
                ("timeframe", timeframe.to_string()),
                ("limit", "10000".to_string()),
            ];
            if let Some(token) = &page_token {
                query_params.push(("page_token", token.clone()));
            }

            let response = self
                .client
                .get(&url)
                .header("APCA-API-KEY-ID", &self.api_key)
                .header("APCA-API-SECRET-KEY", &self.api_secret)
                .query(&query_params)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                anyhow::bail!("Alpaca bars fetch failed: {}", error_text);
            }

            #[derive(Debug, Deserialize)]
            struct BarsResponse {
                bars: Option<std::collections::HashMap<String, Vec<AlpacaBar>>>,
                next_page_token: Option<String>,
            }

            let resp: BarsResponse = response
                .json()
                .await
                .context("Failed to parse Alpaca bars response")?;

            if let Some(bars_map) = resp.bars {
                if let Some(bars) = bars_map.get(symbol) {
                    all_bars.extend(bars.clone());
                }
            }

            match resp.next_page_token {
                Some(token) if !token.is_empty() => {
                    page_token = Some(token);
                    // Add a small delay to respect rate limits if fetching many pages
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                _ => break,
            }
        }

        debug!("Fetched total {} bars for {}", all_bars.len(), symbol);

        // Save to cache
        if !all_bars.is_empty() {
            let mut cache = self.bar_cache.write().unwrap();
            cache.insert(cache_key, all_bars.clone());
        }

        Ok(all_bars)
    }

    pub async fn get_historical_movers(
        &self,
        date: NaiveDate,
        universe: &[String],
    ) -> Result<Vec<String>> {
        info!(
            "MarketScanner: Scanning {} symbols for historical movers on {}",
            universe.len(),
            date
        );

        let mut valid_movers = Vec::new();

        for chunk in universe.chunks(50) {
            let symbols_param = chunk.join(",");

            // Determine endpoint based on asset class
            let (url, timeframe_param) = match self.asset_class {
                AssetClass::Crypto => (
                    format!("{}/v1beta3/crypto/us/bars", self.data_base_url),
                    "1Day", // Crypto API uses different timeframe enum strings sometimes, but 1Day is standard
                ),
                AssetClass::Stock => (format!("{}/v2/stocks/bars", self.data_base_url), "1Day"),
            };

            let start_rfc = format!("{}T00:00:00Z", date);
            let end_rfc = format!("{}T23:59:59Z", date);

            let response = self
                .client
                .get(&url)
                .header("APCA-API-KEY-ID", &self.api_key)
                .header("APCA-API-SECRET-KEY", &self.api_secret)
                .query(&[
                    ("symbols", &symbols_param),
                    ("timeframe", &timeframe_param.to_string()),
                    ("start", &start_rfc),
                    ("end", &end_rfc),
                    ("limit", &"10".to_string()),
                ])
                .send()
                .await
                .context("Failed to fetch historical bars")?;

            if !response.status().is_success() {
                let err = response.text().await.unwrap_or_default();
                error!("MarketScanner: Historical fetch failed: {}", err);
                continue;
            }

            #[derive(Debug, Deserialize)]
            struct MultiBarResponse {
                bars: std::collections::HashMap<String, Vec<AlpacaBar>>,
            }

            let data: MultiBarResponse =
                response.json().await.unwrap_or_else(|_| MultiBarResponse {
                    bars: std::collections::HashMap::new(),
                });

            for (symbol, bars) in data.bars {
                if let Some(bar) = bars.first() {
                    let change_pct = if bar.open != 0.0 {
                        (bar.close - bar.open) / bar.open
                    } else {
                        0.0
                    };
                    let abs_change = change_pct.abs();

                    let is_penny = bar.close < 5.0;
                    let has_volume = bar.volume >= self.min_volume_threshold;

                    if !is_penny && has_volume {
                        valid_movers.push((symbol, abs_change, change_pct));
                    }
                }
            }
        }

        valid_movers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let result: Vec<String> = valid_movers.into_iter().map(|(sym, _, _)| sym).collect();
        info!(
            "MarketScanner: Found {} valid historical movers.",
            result.len()
        );

        Ok(result)
    }

    /// Get top crypto movers by analyzing 24-hour price changes
    /// Since Alpaca doesn't provide a movers API for crypto, we scan a curated universe
    async fn get_crypto_top_movers(&self) -> Result<Vec<String>> {
        let now = chrono::Utc::now();
        let start = now - chrono::Duration::hours(24);
        
        // Crypto bars API expects slash format (BTC/USD), unlike other Alpaca APIs
        let symbols: Vec<String> = CRYPTO_UNIVERSE.iter().map(|s| s.to_string()).collect();
        let symbols_param = symbols.join(",");
        
        info!("MarketScanner: DEBUG - symbols_param = '{}'", symbols_param);
        
       let url = format!("{}/v1beta3/crypto/us/bars", self.data_base_url);
        
        info!(
            "MarketScanner: Fetching 24h bars for {} crypto pairs",
            CRYPTO_UNIVERSE.len()
        );
        
        let response = self
            .client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[
                ("symbols", &symbols_param),
                ("timeframe", &"1Day".to_string()),
                ("start", &start.to_rfc3339()),
                ("end", &now.to_rfc3339()),
                ("limit", &"1".to_string()),
            ])
            .send()
            .await
            .context("Failed to fetch crypto bars for movers detection")?;
        
        if !response.status().is_success() {
            let err = response.text().await.unwrap_or_default();
            error!("MarketScanner: Crypto bars fetch failed: {}", err);
            return Ok(vec![]); // Graceful degradation
        }
        
        #[derive(Debug, Deserialize)]
        struct CryptoBarsResponse {
            bars: std::collections::HashMap<String, Vec<AlpacaBar>>,
        }
        
        let data: CryptoBarsResponse = response
            .json()
            .await
            .context("Failed to parse crypto bars response")?;
        
        info!("MarketScanner: API returned bars for {} symbols", data.bars.len());

        let mut movers: Vec<(String, f64, f64)> = Vec::new();
        
        for (symbol, bars) in data.bars {
            info!("MarketScanner: Processing symbol '{}' with {} bars", symbol, bars.len());
            
            if let Some(bar) = bars.first() {
                // Calculate 24h percentage change
                let change_pct = if bar.open != 0.0 {
                    ((bar.close - bar.open) / bar.open) * 100.0
                } else {
                    0.0
                };
                
                let abs_change = change_pct.abs();
                
                // Filter by volume threshold
                let has_volume = bar.volume >= self.min_volume_threshold;
                
                info!(
                    "MarketScanner: {} - change: {:.2}%, volume: {:.0}, threshold: {:.0}, pass: {}",
                    symbol, change_pct, bar.volume, self.min_volume_threshold, has_volume
                );
                
                if has_volume && abs_change > 0.0 {
                    movers.push((symbol, abs_change, change_pct));
                } else if !has_volume {
                    info!(
                        "MarketScanner: {} FILTERED OUT - volume {:.0} < threshold {:.0}",
                        symbol, bar.volume, self.min_volume_threshold
                    );
                }
            } else {
                info!("MarketScanner: {} - NO BARS in response", symbol);
            }
        }
        
        // Sort by absolute change (descending)
        movers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // Return top 5 movers
        let top_movers: Vec<String> = movers
            .into_iter()
            .take(5)
            .map(|(symbol, abs_change, change_pct)| {
                info!(
                    "MarketScanner: Top mover {} - 24h change: {:.2}% (abs: {:.2}%)",
                    symbol, change_pct, abs_change
                );
                symbol
            })
            .collect();
        
        info!(
            "MarketScanner: Found {} crypto top movers",
            top_movers.len()
        );
        
        Ok(top_movers)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct AlpacaBar {
    #[serde(rename = "t")]
    pub timestamp: String,
    #[serde(rename = "o")]
    pub open: f64,
    #[serde(rename = "h")]
    pub high: f64,
    #[serde(rename = "l")]
    pub low: f64,
    #[serde(rename = "c")]
    pub close: f64,
    #[serde(rename = "v")]
    pub volume: f64,
}

// ===== Execution Service (REST API) =====

pub struct AlpacaExecutionService {
    client: Client,
    api_key: String,
    api_secret: String,
    base_url: String,
    trading_stream: Arc<AlpacaTradingStream>, // Added
    circuit_breaker: Arc<crate::infrastructure::circuit_breaker::CircuitBreaker>, // Circuit breaker for API calls
}

impl AlpacaExecutionService {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        // Configure client with connection pool limits to avoid exhausting API connections
        let client = Client::builder()
            .pool_max_idle_per_host(5) // Limit idle connections per host
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        // Initialize Trading Stream
        let trading_stream = Arc::new(AlpacaTradingStream::new(
            api_key.clone(),
            api_secret.clone(),
            base_url.clone(),
        ));

        // Initialize Circuit Breaker
        let circuit_breaker =
            Arc::new(crate::infrastructure::circuit_breaker::CircuitBreaker::new(
                "AlpacaAPI",
                5,                                  // 5 failures before opening
                2,                                  // 2 successes to close
                std::time::Duration::from_secs(30), // 30s timeout
            ));

        Self {
            client,
            api_key,
            api_secret,
            base_url,
            trading_stream,
            circuit_breaker,
        }
    }
}

#[derive(Debug, Serialize)]
struct AlpacaOrderRequest {
    symbol: String,
    qty: String,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    time_in_force: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_price: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlpacaOrderResponse {
    id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct AlpacaAccount {
    cash: String,
    buying_power: String,
    #[serde(default)]
    daytrade_count: i64,
}

#[derive(Debug, Deserialize)]
struct AlpacaPosition {
    symbol: String,
    qty: String,
    avg_entry_price: String,
    #[serde(default)]
    asset_class: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlpacaOrder {
    id: String,
    symbol: String,
    side: String,
    qty: String,
    #[allow(dead_code)]
    filled_qty: Option<String>,
    filled_avg_price: Option<String>,
    created_at: String,
}

#[async_trait]
impl ExecutionService for AlpacaExecutionService {
    async fn execute(&self, order: Order) -> Result<()> {
        let side_str = match order.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };

        let is_fractional = !order.quantity.fract().is_zero();

        let (type_str, limit_price, stop_price) = match order.order_type {
            crate::domain::trading::types::OrderType::Market => ("market".to_string(), None, None),
            crate::domain::trading::types::OrderType::Limit => {
                ("limit".to_string(), Some(order.price.to_string()), None)
            }
            crate::domain::trading::types::OrderType::Stop => {
                ("stop".to_string(), None, Some(order.price.to_string()))
            }
            crate::domain::trading::types::OrderType::StopLimit => (
                "stop_limit".to_string(),
                Some(order.price.to_string()),
                Some(order.price.to_string()),
            ), // Assuming stop and limit same for simplicity unless we add stop_price to order
        };

        // Alpaca requires 'limit_price' and 'stop_price' fields if type is limit/stop
        // Fractional orders must be market and day? Alpaca restrictions apply.
        // For now, assume standard lots for limit orders or check fractional logic.
        // Usually Limit orders cannot be fractional on Alpaca (requires whole shares? or checks).
        // Safest: if fractional, force market.

        let (final_type, final_limit, final_stop) = if is_fractional && type_str != "market" {
            info!(
                "AlpacaExecution: Forcing MARKET order for fractional quantity {}",
                order.quantity
            );
            ("market".to_string(), None, None)
        } else {
            (type_str, limit_price, stop_price)
        };

        let is_crypto = order.symbol.contains('/') || order.symbol.contains("USD");
        let tif = if is_crypto {
            "gtc"
        } else if is_fractional {
            "day"
        } else {
            "gtc"
        };

        // Using outer AlpacaOrderRequest struct definition

        let order_request = AlpacaOrderRequest {
            symbol: order.symbol.clone(),
            qty: order.quantity.to_string(),
            side: side_str.to_string(),
            order_type: final_type,
            time_in_force: tif.to_string(),
            limit_price: final_limit,
            stop_price: final_stop,
        };

        let url = format!("{}/v2/orders", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .json(&order_request)
            .send()
            .await
            .context("Failed to send order to Alpaca")?;

        if response.status().is_success() {
            let order_resp: AlpacaOrderResponse = response
                .json()
                .await
                .context("Failed to parse Alpaca order response")?;
            info!(
                "Alpaca order placed: {} (status: {})",
                order_resp.id, order_resp.status
            );
            Ok(())
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("Alpaca order failed: {}", error_text)
        }
    }

    async fn get_portfolio(&self) -> Result<crate::domain::trading::portfolio::Portfolio> {
        // Wrap API calls with circuit breaker
        self.circuit_breaker
            .call(async {
                let account_url = format!("{}/v2/account", self.base_url);
                let positions_url = format!("{}/v2/positions", self.base_url);

                // Fetch Account
                let account_resp_raw = self
                    .client
                    .get(&account_url)
                    .header("APCA-API-KEY-ID", &self.api_key)
                    .header("APCA-API-SECRET-KEY", &self.api_secret)
                    .send()
                    .await
                    .context("Failed to send account request")?;

                let account_text = account_resp_raw
                    .text()
                    .await
                    .context("Failed to read account response text")?;
                let account_resp: AlpacaAccount =
                    serde_json::from_str(&account_text).map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to decode Alpaca Account: {}. Body: {}",
                            e,
                            account_text
                        )
                    })?;

                // Fetch Positions
                let positions_resp_raw = self
                    .client
                    .get(&positions_url)
                    .header("APCA-API-KEY-ID", &self.api_key)
                    .header("APCA-API-SECRET-KEY", &self.api_secret)
                    .send()
                    .await
                    .context("Failed to send positions request")?;

                let positions_text = positions_resp_raw
                    .text()
                    .await
                    .context("Failed to read positions response text")?;
                let positions_resp: Vec<AlpacaPosition> = serde_json::from_str(&positions_text)
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to decode Alpaca Positions: {}. Body: {}",
                            e,
                            positions_text
                        )
                    })?;

                let mut portfolio = crate::domain::trading::portfolio::Portfolio::new();
                // Use buying_power or cash? For crypto, buying_power is usually what we have available.
                // Actually, let's log both for debugging.
                let cash = account_resp
                    .cash
                    .parse::<Decimal>()
                    .unwrap_or(Decimal::ZERO);
                let _bp = account_resp
                    .buying_power
                    .parse::<Decimal>()
                    .unwrap_or(Decimal::ZERO);

                /*info!(
                    "Alpaca Account: Cash={}, BuyingPower={}, DayTrades={}",
                    cash, bp, account_resp.daytrade_count
                );*/
                portfolio.cash = cash; // Using cash for now as it's what the validator expects
                portfolio.day_trades_count = account_resp.daytrade_count as u64;

                for alp_pos in positions_resp {
                    let alp_symbol = alp_pos.symbol.clone();

                    // Only normalize crypto symbols (BTCUSD -> BTC/USD)
                    // Stock symbols like GOOGL should remain unchanged
                    let normalized_symbol = if alp_pos.asset_class.as_deref() == Some("crypto") {
                        normalize_crypto_symbol(&alp_symbol).map_err(|e| {
                            anyhow::anyhow!("Symbol normalization failed for {}: {}", alp_symbol, e)
                        })?
                    } else {
                        alp_symbol.clone()
                    };

                    let pos = crate::domain::trading::portfolio::Position {
                        symbol: normalized_symbol.clone(),
                        quantity: alp_pos.qty.parse::<Decimal>().unwrap_or(Decimal::ZERO),
                        average_price: alp_pos
                            .avg_entry_price
                            .parse::<Decimal>()
                            .unwrap_or(Decimal::ZERO),
                    };

                    portfolio.positions.insert(normalized_symbol, pos);
                }

                Ok(portfolio)
            })
            .await
            .map_err(|e| match e {
                crate::infrastructure::circuit_breaker::CircuitBreakerError::Open(msg) => {
                    anyhow::anyhow!("Alpaca API circuit breaker open: {}", msg)
                }
                crate::infrastructure::circuit_breaker::CircuitBreakerError::Inner(inner) => inner,
            })
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let url = format!("{}/v2/orders", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[("status", "open")])
            .send()
            .await
            .context("Failed to fetch open orders")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca open orders fetch failed: {}", error_text);
        }

        let alpaca_orders: Vec<AlpacaOrder> = response
            .json()
            .await
            .context("Failed to parse open orders")?;

        let orders = alpaca_orders
            .into_iter()
            .map(|ao| {
                let side = match ao.side.as_str() {
                    "buy" => OrderSide::Buy,
                    "sell" => OrderSide::Sell,
                    _ => OrderSide::Buy, // Fallback
                };

                let qty = Decimal::from_str_exact(&ao.qty).unwrap_or(Decimal::ZERO);

                Order {
                    id: ao.id,
                    symbol: ao.symbol,
                    side,
                    price: Decimal::ZERO, // Open orders might contain limit price, but simple mapping for now
                    quantity: qty,
                    order_type: crate::domain::trading::types::OrderType::Market, // Simplified
                    timestamp: chrono::DateTime::parse_from_rfc3339(&ao.created_at)
                        .unwrap_or_default()
                        .timestamp(),
                }
            })
            .collect();

        Ok(orders)
    }

    async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let url = format!("{}/v2/orders/{}", self.base_url, order_id);

        let response = self
            .client
            .delete(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to cancel order")?;

        if !response.status().is_success() {
            // If 404, it might already be filled or canceled, consider success or ignore
            if response.status().as_u16() == 404 {
                info!(
                    "AlpacaExecution: Order {} not found for cancellation (already closed?)",
                    order_id
                );
                return Ok(());
            }
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca cancel order failed: {}", error_text);
        }

        info!(
            "AlpacaExecution: Order {} cancelled successfully.",
            order_id
        );
        Ok(())
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        // Only fetch closed orders for today (default status=closed, limit=500)
        // Alpaca defaults to open, use status=closed for "today's filled orders"
        let url = format!("{}/v2/orders", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[("status", "all"), ("limit", "100")]) // Fetch all recent
            .send()
            .await
            .context("Failed to fetch today orders from Alpaca")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca orders fetch failed: {}", error_text);
        }

        let alp_orders: Vec<AlpacaOrder> = response
            .json()
            .await
            .context("Failed to parse Alpaca orders")?;

        let mut orders = Vec::new(); // Fixed mutability
        for ao in alp_orders {
            let side = if ao.side == "buy" {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            };
            let qty = ao.qty.parse::<Decimal>().unwrap_or(Decimal::ZERO);
            let price = ao
                .filled_avg_price
                .as_ref()
                .and_then(|p| p.parse::<Decimal>().ok())
                .unwrap_or(Decimal::ZERO);

            let created_at = chrono::DateTime::parse_from_rfc3339(&ao.created_at)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or(0);

            orders.push(Order {
                id: ao.id,
                symbol: ao.symbol,
                side,
                price,
                quantity: qty,
                order_type: crate::domain::trading::types::OrderType::Market, // Default to Market for history, or infer if possible
                timestamp: created_at,
            });
        }

        Ok(orders)
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        Ok(self.trading_stream.subscribe())
    }
}

#[derive(Debug, Deserialize)]
struct AlpacaAsset {
    #[allow(dead_code)]
    symbol: String,
    #[serde(default)]
    sector: String,
}

pub struct AlpacaSectorProvider {
    client: Client,
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl AlpacaSectorProvider {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            api_secret,
            base_url,
        }
    }
}

#[async_trait]
impl crate::domain::ports::SectorProvider for AlpacaSectorProvider {
    async fn get_sector(&self, symbol: &str) -> Result<String> {
        let url = format!("{}/v2/assets/{}", self.base_url, symbol);

        let response = self
            .client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await?;

        if response.status().is_success() {
            let asset: AlpacaAsset = response.json().await?;
            if asset.sector.is_empty() {
                Ok("Unknown".to_string())
            } else {
                Ok(asset.sector)
            }
        } else {
            Ok("Unknown".to_string())
        }
    }
}
