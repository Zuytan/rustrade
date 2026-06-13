use super::AlpacaMarketDataService;
use crate::config::AssetClass;
use crate::infrastructure::alpaca::common::AlpacaBar;
use crate::infrastructure::core::http_client_factory::build_url_with_query;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use tracing::{debug, error, info, trace};

impl AlpacaMarketDataService {
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

            let data: MultiBarResponse = response
                .json()
                .await
                .context("Failed to parse historical bars response")?;

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

    pub(crate) async fn fetch_historical_bars_internal(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
    ) -> Result<Vec<AlpacaBar>> {
        self.circuit_breaker
            .call(async move {
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

                    let query_pairs: Vec<(&str, String)> =
                        query_params.iter().map(|(k, v)| (*k, v.clone())).collect();
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
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Inner(inner) => {
                    inner
                }
            })
    }

    pub(crate) async fn get_historical_bars_internal(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
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
                            let latest_dt =
                                Utc.timestamp_opt(latest_ts, 0).single().ok_or_else(|| {
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
