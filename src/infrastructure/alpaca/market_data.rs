use super::common::AlpacaBar;
// CRYPTO_UNIVERSE removed - now using dynamic discovery
use super::websocket::AlpacaWebSocketManager;
use crate::config::AssetClass;
use crate::domain::ports::MarketDataService;
use crate::domain::trading::types::MarketEvent;
use crate::infrastructure::core::circuit_breaker::CircuitBreaker;
use crate::infrastructure::core::http_client_factory::{HttpClientFactory, build_url_with_query};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{NaiveDate, TimeZone};
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{
    broadcast,
    mpsc::{self, Receiver},
};
use tracing::{debug, error, info, trace};

// ===== Market Data Service (WebSocket) =====

pub struct AlpacaMarketDataService {
    client: ClientWithMiddleware,
    api_key: String,
    api_secret: String,
    ws_manager: Arc<AlpacaWebSocketManager>,
    data_base_url: String,
    api_base_url: String,
    bar_cache: std::sync::RwLock<std::collections::HashMap<String, Vec<AlpacaBar>>>,
    min_volume_threshold: f64,
    asset_class: AssetClass,
    spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    candle_repository: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
    circuit_breaker: Arc<CircuitBreaker>,
    /// Cache for tradable crypto assets (symbol list + timestamp)
    assets_cache: std::sync::RwLock<Option<(Vec<String>, std::time::Instant)>>,
}

impl AlpacaMarketDataService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api_key: String,
        api_secret: String,
        ws_url: String,
        data_base_url: String,
        api_base_url: String,
        min_volume_threshold: f64,
        asset_class: AssetClass,
        candle_repository: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
    ) -> Self {
        Self::builder()
            .api_key(api_key)
            .api_secret(api_secret)
            .ws_url(ws_url)
            .data_base_url(data_base_url)
            .api_base_url(api_base_url)
            .min_volume_threshold(min_volume_threshold)
            .asset_class(asset_class)
            .candle_repository(candle_repository)
            .build()
    }

    pub fn builder() -> AlpacaMarketDataServiceBuilder {
        AlpacaMarketDataServiceBuilder::default()
    }

    pub fn get_spread_cache(
        &self,
    ) -> Arc<crate::application::market_data::spread_cache::SpreadCache> {
        self.spread_cache.clone()
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

            let (url, timeframe_param) = match self.asset_class {
                AssetClass::Crypto => (
                    format!("{}/v1beta3/crypto/us/bars", self.data_base_url),
                    "1Day",
                ),
                AssetClass::Stock => (format!("{}/v2/stocks/bars", self.data_base_url), "1Day"),
            };

            let start_rfc = format!("{}T00:00:00Z", date);
            let end_rfc = format!("{}T23:59:59Z", date);
            let timeframe_str = timeframe_param.to_string();
            let limit_str = "10".to_string();

            let url_with_query = build_url_with_query(
                &url,
                &[
                    ("symbols", &symbols_param),
                    ("timeframe", &timeframe_str),
                    ("start", &start_rfc),
                    ("end", &end_rfc),
                    ("limit", &limit_str),
                ],
            );

            let response = self
                .client
                .get(&url_with_query)
                .header("APCA-API-KEY-ID", &self.api_key)
                .header("APCA-API-SECRET-KEY", &self.api_secret)
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

                    let is_penny = match self.asset_class {
                        AssetClass::Crypto => false, // Crypto can be < 5.0 (e.g. XRP, ADA)
                        AssetClass::Stock => bar.close < 5.0,
                    };
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

    async fn fetch_historical_bars_internal(
        &self,
        symbol: &str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        timeframe: &str,
    ) -> Result<Vec<AlpacaBar>> {
        self.circuit_breaker.call(async move {
            // 1. Check Cache
            let cache_key = format!(
                "{}:{}:{}:{}",
                symbol,
                start.timestamp(),
                end.timestamp(),
                timeframe
            );

            {
                let cache = match self.bar_cache.read() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("AlpacaMarketDataService: bar_cache lock poisoned during read, recovering");
                        poisoned.into_inner()
                    }
                };
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

            loop {
                let mut query_params = vec![
                    ("symbols", symbol.to_string()),
                    ("start", start.to_rfc3339()),
                    ("end", end.to_rfc3339()),
                    ("timeframe", timeframe.to_string()),
                    ("limit", "10000".to_string()),
                ];

                if !is_crypto {
                    query_params.push(("feed", "iex".to_string()));
                }

                if let Some(token) = &page_token {
                    query_params.push(("page_token", token.clone()));
                }

                debug!(
                    "AlpacaMarketDataService: Fetching {} bars from {} with params: symbol={}, timeframe={}, start={}, end={}",
                    if is_crypto { "crypto" } else { "stock" },
                    url,
                    symbol,
                    timeframe,
                    start,
                    end
                );

                let query_pairs: Vec<(&str, String)> = query_params.iter().map(|(k, v)| (*k, v.clone())).collect();
                let url_with_query = build_url_with_query(&url, &query_pairs);

                let response = self
                    .client
                    .get(&url_with_query)
                    .header("APCA-API-KEY-ID", &self.api_key)
                    .header("APCA-API-SECRET-KEY", &self.api_secret)
                    .send()
                    .await
                    .context("Failed to fetch bars from Alpaca")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_default();
                    error!(
                        "AlpacaMarketDataService: API error {} for {}: {}",
                        status, symbol, error_text
                    );
                    anyhow::bail!("Alpaca API error ({}): {}", status, error_text);
                }

                #[derive(Debug, Deserialize)]
                struct AlpacaBarResponse {
                    bars: std::collections::HashMap<String, Vec<AlpacaBar>>,
                    next_page_token: Option<String>,
                }

                let resp_body: AlpacaBarResponse = response
                    .json()
                    .await
                    .context("Failed to parse bars response")?;

                if let Some(bars) = resp_body.bars.get(symbol) {
                    all_bars.extend(bars.clone());
                }

                page_token = resp_body.next_page_token;
                if page_token.is_none() {
                    break;
                }
            }

            // Update cache before returning
            {
                let mut cache = match self.bar_cache.write() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("AlpacaMarketDataService: bar_cache lock poisoned during write, recovering");
                        poisoned.into_inner()
                    }
                };
                cache.insert(cache_key, all_bars.clone());
            }

            Ok(all_bars)
        })
        .await
        .map_err(|e| match e {
            crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Open(msg) => {
                anyhow::anyhow!("Alpaca Market Data circuit breaker open: {}", msg)
            }
            crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Inner(inner) => inner,
        })
    }

    async fn get_crypto_top_movers(&self, symbols: &[String]) -> Result<Vec<String>> {
        let scanner = crypto_movers::Scanner {
            client: &self.client,
            api_key: &self.api_key,
            api_secret: &self.api_secret,
            base_url: &self.data_base_url,
            min_volume: self.min_volume_threshold,
        };

        scanner.scan(symbols).await
    }
}

#[derive(Default)]
pub struct AlpacaMarketDataServiceBuilder {
    api_key: Option<String>,
    api_secret: Option<String>,
    ws_url: Option<String>,
    data_base_url: Option<String>,
    api_base_url: Option<String>,
    min_volume_threshold: Option<f64>,
    asset_class: Option<AssetClass>,
    candle_repository: Option<Option<Arc<dyn crate::domain::repositories::CandleRepository>>>,
}

impl AlpacaMarketDataServiceBuilder {
    pub fn api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn api_secret(mut self, api_secret: String) -> Self {
        self.api_secret = Some(api_secret);
        self
    }

    pub fn ws_url(mut self, ws_url: String) -> Self {
        self.ws_url = Some(ws_url);
        self
    }

    pub fn data_base_url(mut self, data_base_url: String) -> Self {
        self.data_base_url = Some(data_base_url);
        self
    }

    pub fn api_base_url(mut self, api_base_url: String) -> Self {
        self.api_base_url = Some(api_base_url);
        self
    }

    pub fn min_volume_threshold(mut self, threshold: f64) -> Self {
        self.min_volume_threshold = Some(threshold);
        self
    }

    pub fn asset_class(mut self, asset_class: AssetClass) -> Self {
        self.asset_class = Some(asset_class);
        self
    }

    pub fn candle_repository(
        mut self,
        repo: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
    ) -> Self {
        self.candle_repository = Some(repo);
        self
    }

    pub fn build(self) -> AlpacaMarketDataService {
        let api_key = self.api_key.expect("api_key is required");
        let api_secret = self.api_secret.expect("api_secret is required");
        let ws_url = self.ws_url.expect("ws_url is required");
        let data_base_url = self.data_base_url.expect("data_base_url is required");
        let api_base_url = self.api_base_url.expect("api_base_url is required");
        let min_volume_threshold = self.min_volume_threshold.unwrap_or(100000.0);
        let asset_class = self.asset_class.unwrap_or(AssetClass::Stock);
        let candle_repository = self.candle_repository.flatten();

        let client = HttpClientFactory::create_client();
        let spread_cache =
            Arc::new(crate::application::market_data::spread_cache::SpreadCache::new());
        let ws_manager = Arc::new(AlpacaWebSocketManager::new(
            api_key.clone(),
            api_secret.clone(),
            ws_url,
            spread_cache.clone(),
        ));

        let circuit_breaker = Arc::new(CircuitBreaker::new(
            "AlpacaMarketData",
            5,
            3,
            std::time::Duration::from_secs(60),
        ));

        AlpacaMarketDataService {
            client,
            api_key,
            api_secret,
            ws_manager,
            data_base_url,
            api_base_url,
            bar_cache: std::sync::RwLock::new(std::collections::HashMap::new()),
            min_volume_threshold,
            asset_class,
            spread_cache,
            candle_repository,
            circuit_breaker,
            assets_cache: std::sync::RwLock::new(None),
        }
    }
}

#[async_trait]
impl MarketDataService for AlpacaMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        self.ws_manager.update_subscription(symbols.clone()).await?;
        let mut broadcast_rx = self.ws_manager.subscribe();
        let (tx, rx) = mpsc::channel(100);

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
                        tracing::warn!(
                            "Market event broadcast receiver lagged, missed {} messages",
                            n
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("Market event broadcast channel closed");
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn get_tradable_assets(&self) -> Result<Vec<String>> {
        // For stocks, we don't cache - the movers API returns dynamic list
        if self.asset_class != AssetClass::Crypto {
            // Return empty - stocks use the movers API directly
            return Ok(vec![]);
        }

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

        info!(
            "AlpacaMarketDataService: Fetching tradable crypto assets from {}/v2/assets",
            self.api_base_url
        );

        let url = format!("{}/v2/assets", self.api_base_url);
        let url_with_query =
            build_url_with_query(&url, &[("status", "active"), ("asset_class", "crypto")]);

        let response = self
            .client
            .get(&url_with_query)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to fetch crypto assets from Alpaca")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca assets fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct AlpacaAssetInfo {
            symbol: String,
            tradable: bool,
        }

        let assets: Vec<AlpacaAssetInfo> = response
            .json()
            .await
            .context("Failed to parse Alpaca assets response")?;

        let tradable_symbols: Vec<String> = assets
            .into_iter()
            .filter(|a| a.tradable)
            .map(|a| a.symbol)
            .collect();

        info!(
            "AlpacaMarketDataService: Found {} tradable crypto assets",
            tradable_symbols.len()
        );

        // Update cache
        {
            let mut cache = self
                .assets_cache
                .write()
                .map_err(|e| anyhow::anyhow!("assets cache lock poisoned: {}", e))?;
            *cache = Some((tradable_symbols.clone(), std::time::Instant::now()));
        }

        Ok(tradable_symbols)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        if self.asset_class == AssetClass::Crypto {
            // Get dynamic list of crypto assets
            let crypto_universe = self.get_tradable_assets().await.unwrap_or_default();
            info!(
                "MarketScanner: Scanning crypto top movers from universe of {} pairs",
                crypto_universe.len()
            );
            return self.get_crypto_top_movers(&crypto_universe).await;
        }

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

        let json_val: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse movers JSON")?;

        let movers = response_parser::parse_movers(json_val)?;

        if movers.is_empty() {
            info!("MarketScanner: No movers found in Alpaca response.");
            return Ok(vec![]);
        }

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

        let url = format!("{}/v2/stocks/snapshots", self.data_base_url);
        let symbols_param = candidates.join(",");
        let url_with_query = build_url_with_query(&url, &[("symbols", &symbols_param)]);

        let response = self
            .client
            .get(&url_with_query)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to fetch snapshots for validation")?;

        if !response.status().is_success() {
            let err = response.text().await.unwrap_or_default();
            error!(
                "MarketScanner: Snapshot validation failed: {}. Returning raw candidates.",
                err
            );
            return Ok(candidates);
        }

        let json_val: serde_json::Value = response.json().await?;
        let snapshots = response_parser::parse_snapshots(json_val)?;

        let filtered_symbols: Vec<String> = candidates
            .into_iter()
            .filter(|sym| {
                if let Some(snap) = snapshots.get(sym) {
                    let price = snap.latest_trade.as_ref().map(|t| t.price).unwrap_or(0.0);
                    let volume = snap
                        .daily_bar
                        .as_ref()
                        .map(|b| b.volume)
                        .or_else(|| snap.prev_daily_bar.as_ref().map(|b| b.volume))
                        .unwrap_or(0.0);

                    let is_penny = price < 5.0;
                    let has_volume = volume >= self.min_volume_threshold;

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

        self.circuit_breaker
            .call(async move {
                let is_crypto = symbols.iter().any(|s| s.contains('/'));
                let api_symbols: Vec<String> = if is_crypto {
                    symbols
                        .iter()
                        .map(|s| crate::domain::trading::types::denormalize_crypto_symbol(s))
                        .collect()
                } else {
                    symbols.clone()
                };

                let url = format!("{}/v2/stocks/snapshots", self.data_base_url);
                let symbols_param = api_symbols.join(",");
                let url_with_query = build_url_with_query(&url, &[("symbols", &symbols_param)]);

                let response = self
                    .client
                    .get(&url_with_query)
                    .header("APCA-API-KEY-ID", &self.api_key)
                    .header("APCA-API-SECRET-KEY", &self.api_secret)
                    .send()
                    .await
                    .context("Failed to fetch snapshots from Alpaca")?;

                if !response.status().is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    anyhow::bail!("Alpaca snapshots fetch failed: {}", error_text);
                }

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
                    prev_daily_bar: Option<AlpacaBar>,
                }

                let resp: std::collections::HashMap<String, Snapshot> = response
                    .json()
                    .await
                    .context("Failed to parse Alpaca snapshots response")?;

                let mut prices = std::collections::HashMap::new();

                for (alp_sym, snapshot) in resp {
                    let normalized_sym = if is_crypto {
                        crate::domain::trading::types::normalize_crypto_symbol(&alp_sym)
                            .unwrap_or_else(|_| alp_sym.clone())
                    } else {
                        alp_sym.clone()
                    };

                    let price_f64 = if let Some(trade) = snapshot.latest_trade {
                        trade.price
                    } else if let Some(bar) = snapshot.prev_daily_bar {
                        bar.close
                    } else {
                        0.0
                    };

                    if price_f64 > 0.0
                        && let Some(dec) = rust_decimal::Decimal::from_f64_retain(price_f64)
                    {
                        prices.insert(normalized_sym, dec);
                    }
                }

                Ok(prices)
            })
            .await
            .map_err(|e| match e {
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Open(msg) => {
                    anyhow::anyhow!("Alpaca Market Data circuit breaker open: {}", msg)
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
    ) -> Result<Vec<crate::domain::trading::types::Candle>> {
        const MIN_REQUIRED_BARS: usize = 200;

        if let Some(repo) = &self.candle_repository {
            let start_ts = start.timestamp_millis();
            let end_ts = end.timestamp_millis();

            match repo.get_range(symbol, start_ts, end_ts).await {
                Ok(cached_candles) => {
                    let cached_count = cached_candles.len();

                    if cached_count >= MIN_REQUIRED_BARS {
                        if let Ok(Some(latest_ts)) = repo.get_latest_timestamp(symbol).await {
                            let latest_dt = chrono::Utc
                                .timestamp_opt(latest_ts, 0)
                                .single()
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "Invalid timestamp in candle cache: {}",
                                        latest_ts
                                    )
                                })?;

                            if latest_dt < end && latest_dt >= start {
                                info!(
                                    "AlpacaMarketDataService: Using cached data for {} ({} bars), fetching incremental data from {}",
                                    symbol, cached_count, latest_dt
                                );

                                let new_start = latest_dt + chrono::Duration::seconds(60);
                                let api_result = self
                                    .fetch_historical_bars_internal(
                                        symbol, new_start, end, timeframe,
                                    )
                                    .await;

                                match api_result {
                                    Ok(new_bars) => {
                                        info!(
                                            "AlpacaMarketDataService: Fetched {} new bars from API for {}",
                                            new_bars.len(),
                                            symbol
                                        );

                                        for bar in &new_bars {
                                            let timestamp = chrono::DateTime::parse_from_rfc3339(
                                                &bar.timestamp,
                                            )
                                            .unwrap_or_default()
                                            .timestamp_millis();

                                            let candle = crate::domain::trading::types::Candle {
                                                symbol: symbol.to_string(),
                                                open: Decimal::from_f64_retain(bar.open)
                                                    .unwrap_or(Decimal::ZERO),
                                                high: Decimal::from_f64_retain(bar.high)
                                                    .unwrap_or(Decimal::ZERO),
                                                low: Decimal::from_f64_retain(bar.low)
                                                    .unwrap_or(Decimal::ZERO),
                                                close: Decimal::from_f64_retain(bar.close)
                                                    .unwrap_or(Decimal::ZERO),
                                                volume: Decimal::from_f64_retain(bar.volume)
                                                    .unwrap_or(Decimal::ZERO),
                                                timestamp,
                                            };

                                            if let Err(e) = repo.save(&candle).await {
                                                tracing::warn!(
                                                    "Failed to save candle to repository: {}",
                                                    e
                                                );
                                            }
                                        }

                                        let mut all_candles = cached_candles;
                                        for bar in new_bars {
                                            let timestamp = chrono::DateTime::parse_from_rfc3339(
                                                &bar.timestamp,
                                            )
                                            .unwrap_or_default()
                                            .timestamp_millis();

                                            all_candles.push(
                                                crate::domain::trading::types::Candle {
                                                    symbol: symbol.to_string(),
                                                    open: Decimal::from_f64_retain(bar.open)
                                                        .unwrap_or(Decimal::ZERO),
                                                    high: Decimal::from_f64_retain(bar.high)
                                                        .unwrap_or(Decimal::ZERO),
                                                    low: Decimal::from_f64_retain(bar.low)
                                                        .unwrap_or(Decimal::ZERO),
                                                    close: Decimal::from_f64_retain(bar.close)
                                                        .unwrap_or(Decimal::ZERO),
                                                    volume: Decimal::from_f64_retain(bar.volume)
                                                        .unwrap_or_default(),
                                                    timestamp,
                                                },
                                            );
                                        }

                                        return Ok(all_candles);
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "AlpacaMarketDataService: API fetch failed for {}: {}. Using {} cached bars (DEGRADED MODE)",
                                            symbol,
                                            e,
                                            cached_count
                                        );
                                        return Ok(cached_candles);
                                    }
                                }
                            }
                        }

                        info!(
                            "AlpacaMarketDataService: Using {} cached bars for {} (no API call needed)",
                            cached_count, symbol
                        );
                        return Ok(cached_candles);
                    } else {
                        info!(
                            "AlpacaMarketDataService: Insufficient cache for {} ({}/{} bars), performing full API reload",
                            symbol, cached_count, MIN_REQUIRED_BARS
                        );
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "AlpacaMarketDataService: Cache query failed for {}: {}",
                        symbol,
                        e
                    );
                }
            }
        }

        let api_result = self
            .fetch_historical_bars_internal(symbol, start, end, timeframe)
            .await;

        match api_result {
            Ok(alpaca_bars) => {
                let candles: Vec<_> = alpaca_bars
                    .into_iter()
                    .map(|b| {
                        let timestamp = chrono::DateTime::parse_from_rfc3339(&b.timestamp)
                            .unwrap_or_default()
                            .timestamp_millis();

                        crate::domain::trading::types::Candle {
                            symbol: symbol.to_string(),
                            open: Decimal::from_f64_retain(b.open).unwrap_or(Decimal::ZERO),
                            high: Decimal::from_f64_retain(b.high).unwrap_or(Decimal::ZERO),
                            low: Decimal::from_f64_retain(b.low).unwrap_or(Decimal::ZERO),
                            close: Decimal::from_f64_retain(b.close).unwrap_or(Decimal::ZERO),
                            volume: Decimal::from_f64_retain(b.volume).unwrap_or(Decimal::ZERO),
                            timestamp,
                        }
                    })
                    .collect();

                if let Some(repo) = &self.candle_repository {
                    for candle in &candles {
                        if let Err(e) = repo.save(candle).await {
                            tracing::warn!("Failed to save candle to repository: {}", e);
                        }
                    }
                }

                Ok(candles)
            }
            Err(e) => {
                if let Some(repo) = &self.candle_repository {
                    let start_ts = start.timestamp_millis();
                    let end_ts = end.timestamp_millis();

                    if let Ok(cached_candles) = repo.get_range(symbol, start_ts, end_ts).await
                        && !cached_candles.is_empty()
                    {
                        tracing::warn!(
                            "AlpacaMarketDataService: API failed for {}: {}. Falling back to {} cached bars (DEGRADED MODE)",
                            symbol,
                            e,
                            cached_candles.len()
                        );
                        return Ok(cached_candles);
                    }
                }
                Err(e)
            }
        }
    }
}

// ===== Sector Provider =====

#[derive(Debug, Deserialize)]
struct AlpacaAsset {
    #[serde(default)]
    sector: String,
}

pub struct AlpacaSectorProvider {
    client: ClientWithMiddleware,
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl AlpacaSectorProvider {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        Self {
            client: HttpClientFactory::create_client(),
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

// ===== Internal Modules =====

mod response_parser {
    use super::*;
    use serde_json::Value;

    #[derive(Debug, Deserialize)]
    pub struct Mover {
        pub symbol: String,
    }

    pub fn parse_movers(json: Value) -> Result<Vec<Mover>> {
        let movers: Vec<Mover> = if let Some(gainers) = json.get("gainers") {
            if gainers.is_null() {
                vec![]
            } else {
                serde_json::from_value(gainers.clone()).context("Failed to parse gainers array")?
            }
        } else if let Some(movers_array) = json.as_array() {
            serde_json::from_value(Value::Array(movers_array.clone()))
                .context("Failed to parse movers array")?
        } else {
            vec![]
        };

        Ok(movers)
    }

    #[derive(Debug, Deserialize)]
    pub struct SnapshotTrade {
        #[serde(rename = "p")]
        pub price: f64,
    }

    #[derive(Debug, Deserialize)]
    pub struct SnapshotDay {
        #[serde(rename = "v")]
        pub volume: f64,
    }

    #[derive(Debug, Deserialize)]
    pub struct Snapshot {
        #[serde(rename = "latestTrade")]
        pub latest_trade: Option<SnapshotTrade>,
        #[serde(rename = "dailyBar")]
        pub daily_bar: Option<SnapshotDay>,
        #[serde(rename = "prevDailyBar")]
        pub prev_daily_bar: Option<SnapshotDay>,
    }

    pub fn parse_snapshots(json: Value) -> Result<std::collections::HashMap<String, Snapshot>> {
        serde_json::from_value(json).context("Failed to parse snapshots response")
    }

    #[derive(Debug, Deserialize)]
    pub struct CryptoBarsResponse {
        pub bars: std::collections::HashMap<String, Vec<AlpacaBar>>,
    }

    pub fn parse_crypto_bars(
        json: Value,
    ) -> Result<std::collections::HashMap<String, Vec<AlpacaBar>>> {
        let response: CryptoBarsResponse =
            serde_json::from_value(json).context("Failed to parse crypto bars response")?;
        Ok(response.bars)
    }
}

mod crypto_movers {
    use super::*;

    pub struct Scanner<'a> {
        pub client: &'a ClientWithMiddleware,
        pub api_key: &'a str,
        pub api_secret: &'a str,
        pub base_url: &'a str,
        pub min_volume: f64,
    }

    impl<'a> Scanner<'a> {
        pub async fn scan(&self, symbols: &[String]) -> Result<Vec<String>> {
            if symbols.is_empty() {
                return Ok(vec![]);
            }

            let now = chrono::Utc::now();
            let start = now - chrono::Duration::days(7); // Look back 7 days to find ANY data
            let timeframe_str = "1Day".to_string();
            let start_str = start.to_rfc3339();
            let end_str = now.to_rfc3339();
            let limit_str = "10".to_string();

            let mut all_movers = Vec::new();

            // Batch symbols to avoid URL length limits (approx 40 symbols per batch)
            const BATCH_SIZE: usize = 40;
            for chunk in symbols.chunks(BATCH_SIZE) {
                let symbols_param = chunk.join(",");
                let url = format!("{}/v1beta3/crypto/us/bars", self.base_url);

                let url_with_query = build_url_with_query(
                    &url,
                    &[
                        ("symbols", &symbols_param),
                        ("timeframe", &timeframe_str),
                        ("start", &start_str),
                        ("end", &end_str),
                        ("limit", &limit_str),
                    ],
                );

                let response = match self
                    .client
                    .get(&url_with_query)
                    .header("APCA-API-KEY-ID", self.api_key)
                    .header("APCA-API-SECRET-KEY", self.api_secret)
                    .send()
                    .await
                {
                    Ok(res) => res,
                    Err(e) => {
                        error!("MarketScanner: Crypto bars batch fetch failed: {}", e);
                        continue;
                    }
                };

                if !response.status().is_success() {
                    let err = response.text().await.unwrap_or_default();
                    error!("MarketScanner: Crypto bars fetch failed for batch: {}", err);
                    continue;
                }

                let json_val: serde_json::Value = match response.json().await {
                    Ok(val) => val,
                    Err(e) => {
                        error!("MarketScanner: Failed to parse JSON for batch: {}", e);
                        continue;
                    }
                };

                match response_parser::parse_crypto_bars(json_val) {
                    Ok(bars_map) => {
                        for (symbol, bars) in bars_map {
                            if let Some(bar) = bars.first() {
                                let price_change_pct = (bar.close - bar.open) / bar.open;

                                if bar.volume >= self.min_volume {
                                    all_movers.push((symbol, price_change_pct.abs()));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("MarketScanner: Failed to parse crypto bars: {}", e);
                    }
                }
            }

            all_movers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            info!(
                "MarketScanner: Scanned {} symbols. Found {} with valid data (volume >= {}).",
                symbols.len(),
                all_movers.len(),
                self.min_volume
            );

            let top_movers: Vec<String> = all_movers.into_iter().take(10).map(|(s, _)| s).collect();

            if top_movers.len() < 10 {
                info!(
                    "MarketScanner: Returning top {} movers (less than requested 10).",
                    top_movers.len()
                );
            } else {
                info!("MarketScanner: Returning top 10 movers.");
            }

            Ok(top_movers)
        }
    }
}
