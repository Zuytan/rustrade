//! Binance Market Data Service
//!
//! Provides market data functionality for Binance crypto exchange including:
//! - Real-time WebSocket subscriptions
//! - Historical candle data (klines)
//! - Top movers scanning
//! - Multi-symbol price fetching

// CRYPTO_UNIVERSE removed - now using dynamic discovery
use super::websocket::BinanceWebSocketManager;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::domain::ports::MarketDataService;
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::types::{
    Candle, MarketEvent, denormalize_crypto_symbol, normalize_crypto_symbol,
};
use crate::infrastructure::core::circuit_breaker::CircuitBreaker;
use crate::infrastructure::core::http_client_factory::{HttpClientFactory, build_url_with_query};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{
    broadcast,
    mpsc::{self, Receiver},
};
use tracing::{debug, info, warn};

pub struct BinanceMarketDataService {
    client: ClientWithMiddleware,
    api_key: String,

    base_url: String,
    ws_manager: Arc<BinanceWebSocketManager>,
    spread_cache: Arc<SpreadCache>,
    candle_repository: Option<Arc<dyn CandleRepository>>,
    circuit_breaker: Arc<CircuitBreaker>,
    /// Cache for tradable assets (symbol list + timestamp)
    assets_cache: std::sync::RwLock<Option<(Vec<String>, std::time::Instant)>>,
}

impl BinanceMarketDataService {
    pub fn builder() -> BinanceMarketDataServiceBuilder {
        BinanceMarketDataServiceBuilder::default()
    }

    pub fn get_spread_cache(&self) -> Arc<SpreadCache> {
        self.spread_cache.clone()
    }
}

#[derive(Default)]
pub struct BinanceMarketDataServiceBuilder {
    api_key: Option<String>,

    base_url: Option<String>,
    ws_url: Option<String>,
    candle_repository: Option<Option<Arc<dyn CandleRepository>>>,
}

impl BinanceMarketDataServiceBuilder {
    pub fn api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(base_url);
        self
    }

    pub fn ws_url(mut self, ws_url: String) -> Self {
        self.ws_url = Some(ws_url);
        self
    }

    pub fn candle_repository(mut self, repo: Option<Arc<dyn CandleRepository>>) -> Self {
        self.candle_repository = Some(repo);
        self
    }

    pub fn build(self) -> BinanceMarketDataService {
        let api_key = self.api_key.expect("api_key is required");
        let base_url = self.base_url.expect("base_url is required");
        let ws_url = self.ws_url.expect("ws_url is required");
        let candle_repository = self.candle_repository.flatten();

        let client = HttpClientFactory::create_client();

        let spread_cache = Arc::new(SpreadCache::new());
        let ws_manager = Arc::new(BinanceWebSocketManager::new(ws_url));

        let circuit_breaker = Arc::new(CircuitBreaker::new(
            "BinanceMarketData",
            5,
            3,
            std::time::Duration::from_secs(60),
        ));

        BinanceMarketDataService {
            client,
            api_key,
            base_url,
            ws_manager,
            spread_cache,
            candle_repository,
            circuit_breaker,
            assets_cache: std::sync::RwLock::new(None),
        }
    }
}

#[async_trait]
impl MarketDataService for BinanceMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        // Update subscription on the WebSocket manager
        self.ws_manager.update_subscription(symbols.clone()).await?;

        // Get a broadcast receiver from the manager
        let mut broadcast_rx = self.ws_manager.subscribe();

        // Convert broadcast to mpsc for API compatibility
        let (tx, rx) = mpsc::channel(100);

        // Send subscription events for each symbol
        for symbol in symbols {
            let _ = tx.send(MarketEvent::SymbolSubscription { symbol }).await;
        }

        let tx_forward = tx;

        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event) => {
                        if tx_forward.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            "Binance market event broadcast receiver lagged, missed {} messages",
                            n
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Binance market event broadcast channel closed");
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn get_tradable_assets(&self) -> Result<Vec<String>> {
        // Check cache first (1 hour TTL)
        const CACHE_TTL_SECS: u64 = 3600;
        {
            let cache = self
                .assets_cache
                .read()
                .map_err(|e| anyhow::anyhow!("assets cache lock poisoned: {}", e))?;
            #[allow(clippy::collapsible_if)]
            if let Some((assets, cached_at)) = cache.as_ref() {
                if cached_at.elapsed().as_secs() < CACHE_TTL_SECS {
                    return Ok(assets.clone());
                }
            }
        }

        info!("BinanceMarketDataService: Fetching tradable assets from exchangeInfo");

        let url = format!("{}/api/v3/exchangeInfo", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch exchangeInfo from Binance")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance exchangeInfo fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct SymbolInfo {
            symbol: String,
            status: String,
            #[serde(rename = "quoteAsset")]
            quote_asset: String,
        }

        #[derive(Debug, Deserialize)]
        struct ExchangeInfo {
            symbols: Vec<SymbolInfo>,
        }

        let info: ExchangeInfo = response
            .json()
            .await
            .context("Failed to parse Binance exchangeInfo")?;

        let assets: Vec<String> = info
            .symbols
            .into_iter()
            .filter(|s| s.status == "TRADING" && s.quote_asset == "USDT")
            .filter_map(|s| normalize_crypto_symbol(&s.symbol).ok())
            .collect();

        info!(
            "BinanceMarketDataService: Found {} tradable USDT pairs",
            assets.len()
        );

        // Update cache
        {
            let mut cache = self
                .assets_cache
                .write()
                .map_err(|e| anyhow::anyhow!("assets cache lock poisoned: {}", e))?;
            *cache = Some((assets.clone(), std::time::Instant::now()));
        }

        Ok(assets)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        // Use dynamic asset list instead of static CRYPTO_UNIVERSE
        let all_assets = self.get_tradable_assets().await.unwrap_or_default();
        info!(
            "MarketScanner: Scanning Binance top movers from universe of {} pairs",
            all_assets.len()
        );

        // Binance 24hr ticker endpoint
        let url = format!("{}/api/v3/ticker/24hr", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to fetch 24hr ticker from Binance")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance 24hr ticker fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct Ticker24hr {
            symbol: String,
            #[serde(rename = "priceChangePercent")]
            price_change_percent: String,
            #[serde(rename = "quoteVolume")]
            quote_volume: String,
            #[serde(rename = "lastPrice")]
            last_price: String,
        }

        let tickers: Vec<Ticker24hr> = response
            .json()
            .await
            .context("Failed to parse Binance 24hr ticker response")?;

        // Filter for USDT pairs with high volume
        let mut candidates: Vec<(String, f64, f64)> = tickers
            .into_iter()
            .filter_map(|t| {
                if !t.symbol.ends_with("USDT") {
                    return None;
                }

                let volume = t.quote_volume.parse::<f64>().ok()?;
                let price_change = t.price_change_percent.parse::<f64>().ok()?;
                let price = t.last_price.parse::<f64>().ok()?;

                // Filter: volume > 10M USDT, price > $0.01
                if volume > 10_000_000.0 && price > 0.01 {
                    Some((t.symbol, volume, price_change.abs()))
                } else {
                    None
                }
            })
            .collect();

        // Sort by volume (descending)
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top 10 and normalize symbols
        let mut top_symbols: Vec<String> = candidates
            .into_iter()
            .take(10)
            .filter_map(|(symbol, _, _)| normalize_crypto_symbol(&symbol).ok())
            .collect();

        // Safety enforced limit
        if top_symbols.len() > 10 {
            warn!("MarketScanner: Top symbols logic failed to limit. Force truncating to 10.");
            top_symbols.truncate(10);
        }

        info!(
            "MarketScanner: Final filtered Binance movers: {} (Limited to 10)",
            top_symbols.len()
        );

        Ok(top_symbols)
    }

    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, Decimal>> {
        if symbols.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        self.circuit_breaker
            .call(async move {
                let url = format!("{}/api/v3/ticker/price", self.base_url);

                // Binance allows fetching multiple symbols in one call via [\"BTCUSDT\",\"ETHUSDT\"]
                let api_symbols: Vec<String> = symbols
                    .iter()
                    .map(|s| denormalize_crypto_symbol(s))
                    .collect();

                let symbols_json = serde_json::to_string(&api_symbols)?;
                let url_with_query = build_url_with_query(&url, &[("symbols", &symbols_json)]);

                let response = self
                    .client
                    .get(&url_with_query)
                    .send()
                    .await
                    .context("Failed to fetch prices from Binance")?;

                if !response.status().is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    anyhow::bail!("Binance ticker API error: {}", error_text);
                }

                #[derive(Debug, Deserialize)]
                struct PriceTicker {
                    symbol: String,
                    price: String,
                }

                let tickers: Vec<PriceTicker> = response
                    .json()
                    .await
                    .context("Failed to parse Binance prices")?;

                let mut prices = std::collections::HashMap::new();
                for t in tickers {
                    let normalized = normalize_crypto_symbol(&t.symbol).unwrap_or(t.symbol);
                    if let Ok(p) = Decimal::from_str_exact(&t.price) {
                        prices.insert(normalized, p);
                    }
                }

                Ok(prices)
            })
            .await
            .map_err(|e| match e {
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Open(msg) => {
                    anyhow::anyhow!("Binance Market Data circuit breaker open: {}", msg)
                }
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Inner(inner) => {
                    inner
                }
            })
    }

    async fn get_historical_bars(
        &self,
        symbol: &str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        timeframe: &str,
    ) -> Result<Vec<Candle>> {
        const MIN_REQUIRED_BARS: usize = 200;

        // Check cache first
        if let Some(repo) = &self.candle_repository {
            let start_ts = start.timestamp();
            let end_ts = end.timestamp();

            if let Ok(cached_candles) = repo.get_range(symbol, start_ts, end_ts).await
                && cached_candles.len() >= MIN_REQUIRED_BARS
            {
                info!(
                    "BinanceMarketDataService: Using {} cached bars for {}",
                    cached_candles.len(),
                    symbol
                );
                return Ok(cached_candles);
            }
        }

        // Fetch from API
        let candles = self
            .fetch_historical_bars_internal(symbol, start, end, timeframe)
            .await?;

        // Save to cache
        if let Some(repo) = &self.candle_repository {
            for candle in &candles {
                if let Err(e) = repo.save(candle).await {
                    warn!("Failed to save candle to repository: {}", e);
                }
            }
        }

        Ok(candles)
    }
}

impl BinanceMarketDataService {
    async fn fetch_historical_bars_internal(
        &self,
        symbol: &str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        timeframe: &str,
    ) -> Result<Vec<Candle>> {
        self.circuit_breaker
            .call(async move {
                // Denormalize symbol
                let api_symbol = denormalize_crypto_symbol(symbol);

                // Convert timeframe (e.g., "1Min" -> "1m")
                let interval = match timeframe {
                    "1Min" => "1m",
                    "5Min" => "5m",
                    "15Min" => "15m",
                    "1Hour" => "1h",
                    "1Day" => "1d",
                    _ => "1m",
                };

                let url = format!("{}/api/v3/klines", self.base_url);

                let start_ms = start.timestamp_millis();
                let end_ms = end.timestamp_millis();

                let start_ms_str = start_ms.to_string();
                let end_ms_str = end_ms.to_string();

                let url_with_query = build_url_with_query(
                    &url,
                    &[
                        ("symbol", api_symbol.as_str()),
                        ("interval", interval),
                        ("startTime", &start_ms_str),
                        ("endTime", &end_ms_str),
                        ("limit", "1000"),
                    ],
                );

                let response = self
                    .client
                    .get(&url_with_query)
                    .header("X-MBX-APIKEY", &self.api_key)
                    .send()
                    .await
                    .context("Failed to fetch klines from Binance")?;

                if !response.status().is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    anyhow::bail!("Binance klines fetch failed: {}", error_text);
                }

                // Binance klines format: [timestamp, open, high, low, close, volume, ...]
                let klines: Vec<serde_json::Value> = response
                    .json()
                    .await
                    .context("Failed to parse Binance klines response")?;

                let candles: Vec<Candle> = klines
                    .into_iter()
                    .filter_map(|k| {
                        let arr = k.as_array()?;
                        if arr.len() < 6 {
                            return None;
                        }

                        let timestamp = arr[0].as_i64()?;

                        let open = arr[1].as_str()?.parse::<f64>().ok()?;
                        let high = arr[2].as_str()?.parse::<f64>().ok()?;
                        let low = arr[3].as_str()?.parse::<f64>().ok()?;
                        let close = arr[4].as_str()?.parse::<f64>().ok()?;
                        let volume = arr[5].as_str()?.parse::<f64>().ok()?;

                        Some(Candle {
                            symbol: symbol.to_string(),
                            open: Decimal::from_f64_retain(open).unwrap_or(Decimal::ZERO),
                            high: Decimal::from_f64_retain(high).unwrap_or(Decimal::ZERO),
                            low: Decimal::from_f64_retain(low).unwrap_or(Decimal::ZERO),
                            close: Decimal::from_f64_retain(close).unwrap_or(Decimal::ZERO),
                            volume: Decimal::from_f64_retain(volume).unwrap_or(Decimal::ZERO),
                            timestamp,
                        })
                    })
                    .collect();

                info!(
                    "BinanceMarketDataService: Fetched {} bars for {}",
                    candles.len(),
                    symbol
                );

                Ok(candles)
            })
            .await
            .map_err(|e| match e {
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Open(msg) => {
                    anyhow::anyhow!("Binance Market Data circuit breaker open: {}", msg)
                }
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Inner(inner) => {
                    inner
                }
            })
    }
}
