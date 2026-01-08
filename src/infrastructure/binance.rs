use crate::domain::ports::OrderUpdate;
use crate::domain::ports::{ExecutionService, MarketDataService, SectorProvider};
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{
    denormalize_crypto_symbol, normalize_crypto_symbol, MarketEvent, Order, OrderSide,
    OrderType,
};
use crate::infrastructure::binance_websocket::BinanceWebSocketManager;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::TimeZone;
use hmac::{Hmac, Mac};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::sync::Arc;
use tokio::sync::{
    broadcast,
    mpsc::{self, Receiver},
};
use tracing::{debug, info, warn};

// ===== Constants =====

/// Major crypto pairs to scan for top movers on Binance
const CRYPTO_UNIVERSE: &[&str] = &[
    "BTC/USDT",
    "ETH/USDT",
    "BNB/USDT",
    "SOL/USDT",
    "ADA/USDT",
    "XRP/USDT",
    "DOT/USDT",
    "AVAX/USDT",
    "MATIC/USDT",
    "LINK/USDT",
];

// ===== Market Data Service =====

pub struct BinanceMarketDataService {
    client: Client,
    api_key: String,
    #[allow(dead_code)]
    api_secret: String,
    base_url: String,
    ws_manager: Arc<BinanceWebSocketManager>,
    spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    candle_repository: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
}

impl BinanceMarketDataService {
    pub fn builder() -> BinanceMarketDataServiceBuilder {
        BinanceMarketDataServiceBuilder::default()
    }

    pub fn get_spread_cache(
        &self,
    ) -> Arc<crate::application::market_data::spread_cache::SpreadCache> {
        self.spread_cache.clone()
    }
}

#[derive(Default)]
pub struct BinanceMarketDataServiceBuilder {
    api_key: Option<String>,
    api_secret: Option<String>,
    base_url: Option<String>,
    ws_url: Option<String>,
    candle_repository: Option<Option<Arc<dyn crate::domain::repositories::CandleRepository>>>,
}

impl BinanceMarketDataServiceBuilder {
    pub fn api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn api_secret(mut self, api_secret: String) -> Self {
        self.api_secret = Some(api_secret);
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

    pub fn candle_repository(
        mut self,
        repo: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
    ) -> Self {
        self.candle_repository = Some(repo);
        self
    }

    pub fn build(self) -> BinanceMarketDataService {
        let api_key = self.api_key.expect("api_key is required");
        let api_secret = self.api_secret.expect("api_secret is required");
        let base_url = self.base_url.expect("base_url is required");
        let ws_url = self.ws_url.expect("ws_url is required");
        let candle_repository = self.candle_repository.flatten();

        let client = Client::builder()
            .pool_max_idle_per_host(5)
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        let spread_cache =
            Arc::new(crate::application::market_data::spread_cache::SpreadCache::new());
        let ws_manager = Arc::new(BinanceWebSocketManager::new(
            api_key.clone(),
            ws_url,
            spread_cache.clone(),
        ));

        BinanceMarketDataService {
            client,
            api_key,
            api_secret,
            base_url,
            ws_manager,
            spread_cache,
            candle_repository,
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
                        warn!("Binance market event broadcast receiver lagged, missed {} messages", n);
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

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        info!(
            "MarketScanner: Scanning Binance top movers from universe of {} pairs",
            CRYPTO_UNIVERSE.len()
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
        let top_symbols: Vec<String> = candidates
            .into_iter()
            .take(10)
            .filter_map(|(symbol, _, _)| normalize_crypto_symbol(&symbol).ok())
            .collect();

        info!(
            "MarketScanner: Final filtered Binance movers: {}",
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

        // Denormalize symbols for Binance API (BTC/USDT -> BTCUSDT)
        let api_symbols: Vec<String> = symbols
            .iter()
            .map(|s| denormalize_crypto_symbol(s))
            .collect();

        let url = format!("{}/api/v3/ticker/price", self.base_url);

        // Build JSON array for symbols parameter
        let symbols_json = serde_json::to_string(&api_symbols)?;

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("symbols", symbols_json)])
            .send()
            .await
            .context("Failed to fetch prices from Binance")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance price fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct PriceTicker {
            symbol: String,
            price: String,
        }

        let price_tickers: Vec<PriceTicker> = response
            .json()
            .await
            .context("Failed to parse Binance price response")?;

        let mut prices = std::collections::HashMap::new();

        for ticker in price_tickers {
            // Normalize symbol back (BTCUSDT -> BTC/USDT)
            let normalized_sym = normalize_crypto_symbol(&ticker.symbol).unwrap_or(ticker.symbol);

            if let Ok(price_f64) = ticker.price.parse::<f64>()
                && let Some(dec) = Decimal::from_f64_retain(price_f64) {
                    prices.insert(normalized_sym, dec);
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
        const MIN_REQUIRED_BARS: usize = 200;

        // Check cache first
        if let Some(repo) = &self.candle_repository {
            let start_ts = start.timestamp();
            let end_ts = end.timestamp();

            if let Ok(cached_candles) = repo.get_range(symbol, start_ts, end_ts).await
                && cached_candles.len() >= MIN_REQUIRED_BARS {
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
    ) -> Result<Vec<crate::domain::trading::types::Candle>> {
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

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[
                ("symbol", api_symbol.as_str()),
                ("interval", interval),
                ("startTime", &start_ms.to_string()),
                ("endTime", &end_ms.to_string()),
                ("limit", "1000"),
            ])
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

        let candles: Vec<crate::domain::trading::types::Candle> = klines
            .into_iter()
            .filter_map(|k| {
                let arr = k.as_array()?;
                if arr.len() < 6 {
                    return None;
                }

                let timestamp_ms = arr[0].as_i64()?;
                let timestamp = timestamp_ms / 1000;

                let open = arr[1].as_str()?.parse::<f64>().ok()?;
                let high = arr[2].as_str()?.parse::<f64>().ok()?;
                let low = arr[3].as_str()?.parse::<f64>().ok()?;
                let close = arr[4].as_str()?.parse::<f64>().ok()?;
                let volume = arr[5].as_str()?.parse::<f64>().ok()?;

                Some(crate::domain::trading::types::Candle {
                    symbol: symbol.to_string(),
                    open: Decimal::from_f64_retain(open).unwrap_or(Decimal::ZERO),
                    high: Decimal::from_f64_retain(high).unwrap_or(Decimal::ZERO),
                    low: Decimal::from_f64_retain(low).unwrap_or(Decimal::ZERO),
                    close: Decimal::from_f64_retain(close).unwrap_or(Decimal::ZERO),
                    volume,
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
    }
}

// ===== Execution Service =====

pub struct BinanceExecutionService {
    client: Client,
    api_key: String,
    api_secret: String,
    base_url: String,
    order_update_tx: broadcast::Sender<OrderUpdate>,
}

impl BinanceExecutionService {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        let (order_update_tx, _) = broadcast::channel(100);

        Self {
            client,
            api_key,
            api_secret,
            base_url,
            order_update_tx,
        }
    }

    /// Generate HMAC-SHA256 signature for Binance API requests
    fn sign_request(&self, query_string: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

#[async_trait]
impl ExecutionService for BinanceExecutionService {
    async fn execute(&self, order: Order) -> Result<()> {
        let api_symbol = denormalize_crypto_symbol(&order.symbol);

        // Binance side
        let side = match order.side {
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        };

        // Binance order type
        let order_type = match order.order_type {
            OrderType::Market => "MARKET",
            OrderType::Limit => "LIMIT",
            OrderType::Stop => "STOP_LOSS",
            OrderType::StopLimit => "STOP_LOSS_LIMIT",
        };

        let timestamp = chrono::Utc::now().timestamp_millis();

        // Build query string
        let mut params = vec![
            ("symbol", api_symbol.clone()),
            ("side", side.to_string()),
            ("type", order_type.to_string()),
            ("quantity", order.quantity.to_string()),
            ("timestamp", timestamp.to_string()),
        ];

        if let OrderType::Limit = order.order_type
            && order.price > Decimal::ZERO {
                params.push(("price", order.price.to_string()));
                params.push(("timeInForce", "GTC".to_string()));
            }

        let query_string: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        let signature = self.sign_request(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = format!("{}/api/v3/order?{}", self.base_url, signed_query);

        let response = self
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to place order on Binance")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance order placement failed: {}", error_text);
        }

        let response_json: serde_json::Value = response.json().await?;
        info!(
            "Binance order placed successfully: {:?}",
            response_json
        );

        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let query_string = format!("timestamp={}", timestamp);
        let signature = self.sign_request(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = format!("{}/api/v3/account?{}", self.base_url, signed_query);

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to fetch account from Binance")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!(
                "Binance account fetch failed - Status: {}, URL: {}, Response: {}",
                status, url, error_text
            );
            anyhow::bail!("Binance account fetch failed: {} - {}", status, error_text);
        }

        #[derive(Debug, Deserialize)]
        struct Balance {
            asset: String,
            free: String,
            locked: String,
        }

        #[derive(Debug, Deserialize)]
        struct Account {
            balances: Vec<Balance>,
        }

        let account: Account = response.json().await?;

        let mut portfolio = Portfolio::new();

        // Find USDT balance for cash
        for balance in &account.balances {
            if balance.asset == "USDT" {
                let free = balance.free.parse::<f64>().unwrap_or(0.0);
                portfolio.cash = Decimal::from_f64_retain(free).unwrap_or(Decimal::ZERO);
                break;
            }
        }

        // Add positions for non-zero balances (excluding USDT)
        for balance in account.balances {
            if balance.asset == "USDT" {
                continue;
            }

            let free = balance.free.parse::<f64>().unwrap_or(0.0);
            let locked = balance.locked.parse::<f64>().unwrap_or(0.0);
            let total = free + locked;

            if total > 0.0 {
                let symbol = format!("{}/USDT", balance.asset);
                let quantity = Decimal::from_f64_retain(total).unwrap_or(Decimal::ZERO);

                portfolio.positions.insert(
                    symbol.clone(),
                    crate::domain::trading::portfolio::Position {
                        symbol,
                        quantity,
                        average_price: Decimal::ZERO, // Binance doesn't provide avg price in account endpoint
                    },
                );
            }
        }

        Ok(portfolio)
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        // Get orders from start of today (UTC)
        let today_start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let _start_time = chrono::Utc
            .from_utc_datetime(&today_start)
            .timestamp_millis();

        // Note: Binance requires symbol for allOrders endpoint
        // For simplicity, return empty for now (can be enhanced to query all symbols)
        warn!("BinanceExecutionService::get_today_orders not fully implemented - requires symbol parameter");
        Ok(vec![])
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let query_string = format!("timestamp={}", timestamp);
        let signature = self.sign_request(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = format!("{}/api/v3/openOrders?{}", self.base_url, signed_query);

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to fetch open orders from Binance")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance open orders fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct BinanceOrder {
            symbol: String,
            #[serde(rename = "orderId")]
            order_id: i64,
            side: String,
            #[serde(rename = "type")]
            order_type: String,
            #[serde(rename = "origQty")]
            orig_qty: String,
            price: String,
        }

        let binance_orders: Vec<BinanceOrder> = response.json().await?;

        let orders: Vec<Order> = binance_orders
            .into_iter()
            .filter_map(|bo| {
                let symbol = normalize_crypto_symbol(&bo.symbol).ok()?;
                let side = match bo.side.as_str() {
                    "BUY" => OrderSide::Buy,
                    "SELL" => OrderSide::Sell,
                    _ => return None,
                };
                let order_type = match bo.order_type.as_str() {
                    "MARKET" => OrderType::Market,
                    "LIMIT" => OrderType::Limit,
                    _ => return None,
                };
                let quantity = Decimal::from_f64_retain(bo.orig_qty.parse().ok()?)
                    .unwrap_or(Decimal::ZERO);
                let price = if order_type == OrderType::Limit {
                    Decimal::from_f64_retain(bo.price.parse().ok()?).unwrap_or(Decimal::ZERO)
                } else {
                    Decimal::ZERO
                };

                Some(Order {
                    id: bo.order_id.to_string(),
                    symbol,
                    side,
                    order_type,
                    quantity,
                    price,
                    timestamp: chrono::Utc::now().timestamp(),
                })
            })
            .collect();

        Ok(orders)
    }

    async fn cancel_order(&self, _order_id: &str) -> Result<()> {
        // Note: Binance requires symbol for order cancellation
        // For now, return error (can be enhanced to store symbol mapping)
        anyhow::bail!("BinanceExecutionService::cancel_order requires symbol - not implemented");
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        // TODO: Implement User Data Stream for order updates
        // This requires:
        // 1. POST /api/v3/userDataStream to get listenKey
        // 2. Connect WebSocket to wss://stream.binance.com:9443/ws/<listenKey>
        // 3. Keep listenKey alive with PUT requests every 30 minutes
        // 4. Parse executionReport events and broadcast as OrderUpdate

        Ok(self.order_update_tx.subscribe())
    }
}

// ===== Sector Provider =====

pub struct BinanceSectorProvider;

#[async_trait]
impl SectorProvider for BinanceSectorProvider {
    async fn get_sector(&self, symbol: &str) -> Result<String> {
        // Map crypto symbols to categories
        let sector = if symbol.starts_with("BTC") || symbol.starts_with("ETH") {
            "Layer1"
        } else if symbol.starts_with("UNI")
            || symbol.starts_with("AAVE")
            || symbol.starts_with("LINK")
        {
            "DeFi"
        } else if symbol.starts_with("SOL") || symbol.starts_with("AVAX") || symbol.starts_with("DOT") {
            "Layer1"
        } else if symbol.starts_with("MATIC") {
            "Layer2"
        } else if symbol.starts_with("USDT") || symbol.starts_with("USDC") {
            "Stablecoin"
        } else {
            "Other"
        };

        Ok(sector.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binance_symbol_denormalization() {
        assert_eq!(denormalize_crypto_symbol("BTC/USDT"), "BTCUSDT");
        assert_eq!(denormalize_crypto_symbol("ETH/USDT"), "ETHUSDT");
        assert_eq!(denormalize_crypto_symbol("AVAX/USDT"), "AVAXUSDT");
    }

    #[test]
    fn test_binance_symbol_normalization() {
        assert_eq!(normalize_crypto_symbol("BTCUSDT").unwrap(), "BTC/USDT");
        assert_eq!(normalize_crypto_symbol("ETHUSDT").unwrap(), "ETH/USDT");
        assert_eq!(normalize_crypto_symbol("BNBUSDT").unwrap(), "BNB/USDT");
    }

    #[test]
    fn test_hmac_signature_format() {
        let service = BinanceExecutionService::new(
            "test_key".to_string(),
            "test_secret".to_string(),
            "https://api.binance.com".to_string(),
        );

        let signature = service.sign_request("symbol=BTCUSDT&side=BUY&type=MARKET&quantity=0.001&timestamp=1234567890");
        
        // Verify signature is 64 hex characters
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
