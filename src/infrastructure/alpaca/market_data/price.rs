use super::AlpacaMarketDataService;
use crate::config::AssetClass;
use crate::infrastructure::alpaca::common::AlpacaBar;
use crate::infrastructure::core::http_client_factory::build_url_with_query;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::Deserialize;
use tracing::{error, info};

impl AlpacaMarketDataService {
    pub(crate) async fn get_crypto_top_movers(&self, symbols: &[String]) -> Result<Vec<String>> {
        let scanner = super::crypto_movers::Scanner {
            client: &self.client,
            api_key: &self.api_key,
            api_secret: &self.api_secret,
            base_url: &self.data_base_url,
            min_volume: self.min_volume_threshold,
        };

        scanner.scan(symbols).await
    }

    pub(crate) async fn get_tradable_assets_internal(&self) -> Result<Vec<String>> {
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

    pub(crate) async fn get_top_movers_internal(&self) -> Result<Vec<String>> {
        if self.asset_class == AssetClass::Crypto {
            // Get dynamic list of crypto assets
            let crypto_universe = self
                .get_tradable_assets_internal()
                .await
                .unwrap_or_default();
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

        let movers = super::response_parser::parse_movers(json_val)?;

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
        let snapshots = super::response_parser::parse_snapshots(json_val)?;

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

    pub(crate) async fn get_prices_internal(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, Decimal>> {
        if symbols.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        self.circuit_breaker
            .call(async move {
                let is_crypto = symbols.iter().any(|s| s.contains('/'));
                let api_symbols: Vec<String> = symbols.clone();

                let url = if is_crypto {
                    format!("{}/v1beta3/crypto/us/snapshots", self.data_base_url)
                } else {
                    format!("{}/v2/stocks/snapshots", self.data_base_url)
                };
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
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_default();
                    error!(
                        "Alpaca API Error [{}]: URL: {} | Symbols: {} | Response: {}",
                        status, url_with_query, symbols_param, error_text
                    );
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

                let json_val: serde_json::Value = response
                    .json()
                    .await
                    .context("Failed to parse Alpaca snapshots response")?;

                let resp: std::collections::HashMap<String, Snapshot> = if is_crypto {
                    if let Some(snapshots_obj) = json_val.get("snapshots") {
                        serde_json::from_value(snapshots_obj.clone())
                            .context("Failed to parse crypto snapshots")?
                    } else {
                        std::collections::HashMap::new()
                    }
                } else {
                    serde_json::from_value(json_val).context("Failed to parse stock snapshots")?
                };

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
                        && let Some(dec) = Decimal::from_f64_retain(price_f64)
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
}
