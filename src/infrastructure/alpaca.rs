use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::types::{MarketEvent, Order, OrderSide};
use crate::infrastructure::alpaca_websocket::AlpacaWebSocketManager;
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver};
use tracing::info;

// ===== Market Data Service (WebSocket) =====

pub struct AlpacaMarketDataService {
    client: Client,
    api_key: String,
    api_secret: String,
    ws_manager: Arc<AlpacaWebSocketManager>, // Singleton WebSocket manager
}

impl AlpacaMarketDataService {
    pub fn new(api_key: String, api_secret: String, ws_url: String) -> Self {
        // Configure client with connection pool limits
        let client = Client::builder()
            .pool_max_idle_per_host(5)
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        // Create singleton WebSocket manager
        let ws_manager = Arc::new(AlpacaWebSocketManager::new(
            api_key.clone(),
            api_secret.clone(),
            ws_url,
        ));

        Self {
            client,
            api_key,
            api_secret,
            ws_manager,
        }
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
            while let Ok(event) = broadcast_rx.recv().await {
                if tx.send(event).await.is_err() {
                    // Receiver dropped, exit
                    break;
                }
            }
        });

        Ok(rx)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        // Alpaca Data v1beta1 Screener Movers endpoint
        // v2/stocks/movers is often not found or requires specific tier.
        let url = "https://data.alpaca.markets/v1beta1/screener/stocks/movers";

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
            let v2_url = "https://data.alpaca.markets/v2/stocks/movers";
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
            price: f64, // Optional in some V2 responses or might be named differently
        }

        // V2 response format can differ, it sometimes returns a list directly or a different field.
        // Actually Screener v1beta1 returns { gainers: [...] }.
        // V2 Movers returns [Mover, Mover, ...] or a struct?
        // Let's be smart about deserialization.

        let json_val: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse movers JSON")?;

        // Detailed logging of the raw response for debugging
        // info!("Alpaca movers raw response: {}", json_val);

        let movers: Vec<Mover> = if let Some(gainers) = json_val.get("gainers") {
            if gainers.is_null() {
                info!("Alpaca movers: 'gainers' field is null. No movers found.");
                vec![]
            } else {
                serde_json::from_value(gainers.clone())?
            }
        } else if let Some(movers) = json_val.as_array() {
            serde_json::from_value(serde_json::Value::Array(movers.clone()))?
        } else {
            info!(
                "Alpaca movers: No 'gainers' or array found in response. JSON: {}",
                json_val
            );
            vec![]
        };

        if movers.is_empty() {
            info!("MarketScanner: No movers found in Alpaca response.");
        }

        let symbols = movers
            .into_iter()
            .filter(|m| {
                // If price is 0.0 (missing in some V2 responses), we don't confirm it's a penny stock
                let is_penny = m.price > 0.0 && m.price < 5.0;
                let is_warrant = m.symbol.contains(".WS") || m.symbol.ends_with('W');
                let is_unit = m.symbol.ends_with('U');

                let keep = !is_penny && !is_warrant && !is_unit;
                if !keep {
                    info!(
                        "MarketScanner: Filtering out {} (price: {:.2}, warrant: {}, unit: {})",
                        m.symbol, m.price, is_warrant, is_unit
                    );
                }
                keep
            })
            .map(|m| m.symbol)
            .collect();

        info!("MarketScanner: Final filtered movers list: {:?}", symbols);
        Ok(symbols)
    }

    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, rust_decimal::Decimal>> {
        if symbols.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let url = "https://data.alpaca.markets/v2/stocks/snapshots";
        // Join symbols with comma
        let symbols_param = symbols.join(",");

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

        for (sym, snapshot) in resp {
            let price_f64 = if let Some(trade) = snapshot.latest_trade {
                trade.price
            } else if let Some(bar) = snapshot.prev_daily_bar {
                bar.close
            } else {
                0.0
            };

            if price_f64 > 0.0 {
                if let Some(dec) = rust_decimal::Decimal::from_f64_retain(price_f64) {
                    prices.insert(sym, dec);
                }
            }
        }

        Ok(prices)
    }
}

impl AlpacaMarketDataService {
    pub async fn get_historical_bars(
        &self,
        symbol: &str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        timeframe: &str,
    ) -> Result<Vec<AlpacaBar>> {
        let url = "https://data.alpaca.markets/v2/stocks/bars";
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

            if let Some(token) = &page_token {
                query_params.push(("page_token", token.clone()));
            }

            let response = self
                .client
                .get(url)
                .header("APCA-API-KEY-ID", &self.api_key)
                .header("APCA-API-SECRET-KEY", &self.api_secret)
                .query(&query_params)
                .send()
                .await
                .context("Failed to fetch historical bars")?;

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

        info!("Fetched total {} bars for {}", all_bars.len(), symbol);
        Ok(all_bars)
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
    pub volume: u64,
}

// ===== Execution Service (REST API) =====

pub struct AlpacaExecutionService {
    client: Client,
    api_key: String,
    api_secret: String,
    base_url: String,
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

        Self {
            client,
            api_key,
            api_secret,
            base_url,
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
}

#[derive(Debug, Deserialize)]
struct AlpacaPosition {
    symbol: String,
    qty: String,
    avg_entry_price: String,
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
             crate::domain::types::OrderType::Market => ("market".to_string(), None, None),
             crate::domain::types::OrderType::Limit => ("limit".to_string(), Some(order.price.to_string()), None),
             crate::domain::types::OrderType::Stop => ("stop".to_string(), None, Some(order.price.to_string())),
             crate::domain::types::OrderType::StopLimit => ("stop_limit".to_string(), Some(order.price.to_string()), Some(order.price.to_string())), // Assuming stop and limit same for simplicity unless we add stop_price to order
        };
        
        // Alpaca requires 'limit_price' and 'stop_price' fields if type is limit/stop
        // Fractional orders must be market and day? Alpaca restrictions apply.
        // For now, assume standard lots for limit orders or check fractional logic.
        // Usually Limit orders cannot be fractional on Alpaca (requires whole shares? or checks).
        // Safest: if fractional, force market.
        
        let (final_type, final_limit, final_stop) = if is_fractional && type_str != "market" {
             info!("AlpacaExecution: Forcing MARKET order for fractional quantity {}", order.quantity);
             ("market".to_string(), None, None)
        } else {
             (type_str, limit_price, stop_price)
        };

        let tif = if is_fractional { "day" } else { "gtc" };

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

    async fn get_portfolio(&self) -> Result<crate::domain::portfolio::Portfolio> {
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
        let account_resp: AlpacaAccount = serde_json::from_str(&account_text).map_err(|e| {
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
        let positions_resp: Vec<AlpacaPosition> =
            serde_json::from_str(&positions_text).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to decode Alpaca Positions: {}. Body: {}",
                    e,
                    positions_text
                )
            })?;

        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        // Use buying_power or cash? For crypto, buying_power is usually what we have available.
        // Actually, let's log both for debugging.
        let cash = account_resp
            .cash
            .parse::<Decimal>()
            .unwrap_or(Decimal::ZERO);
        let bp = account_resp
            .buying_power
            .parse::<Decimal>()
            .unwrap_or(Decimal::ZERO);

        info!("Alpaca Account: Cash={}, BuyingPower={}", cash, bp);
        portfolio.cash = cash; // Using cash for now as it's what the validator expects

        for alp_pos in positions_resp {
            // Normalize symbol: Alpaca might return BTCUSD or BTC/USD.
            // We strip any / to be consistent if needed, or just keep it.
            // Let's try to match exactly first, but log if it's different.
            let alp_symbol = alp_pos.symbol.clone();
            let pos = crate::domain::portfolio::Position {
                symbol: alp_symbol.clone(),
                quantity: alp_pos.qty.parse::<Decimal>().unwrap_or(Decimal::ZERO),
                average_price: alp_pos
                    .avg_entry_price
                    .parse::<Decimal>()
                    .unwrap_or(Decimal::ZERO),
            };

            // Log positions for debugging
            info!("Alpaca Position: {} qty={}", alp_symbol, pos.quantity);

            // Store with and without slash to be safe?
            // Better: use a normalized key in the map or normalize during lookup.
            portfolio.positions.insert(alp_symbol, pos);
        }

        Ok(portfolio)
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        let now = chrono::Utc::now();
        // Start of today (UTC)
        let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();

        let url = format!("{}/v2/orders", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[("status", "all"), ("after", &today_start.to_rfc3339())])
            .send()
            .await
            .context("Failed to fetch orders from Alpaca")?;

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
                order_type: crate::domain::types::OrderType::Market, // Default to Market for history, or infer if possible
                timestamp: created_at,
            });
        }

        Ok(orders)
    }
}
