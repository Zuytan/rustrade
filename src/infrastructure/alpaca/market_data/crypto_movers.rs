use super::response_parser;
use crate::infrastructure::core::http_client_factory::build_url_with_query;
use anyhow::Result;
use reqwest_middleware::ClientWithMiddleware;
use tracing::{error, info};

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
