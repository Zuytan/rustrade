use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::types::{MarketEvent, Order, OrderSide};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, Receiver};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{error, info, warn};

// ===== Market Data Service (WebSocket) =====

pub struct AlpacaMarketDataService {
    client: Client,
    api_key: String,
    api_secret: String,
    ws_url: String,
}

impl AlpacaMarketDataService {
    pub fn new(api_key: String, api_secret: String, ws_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            api_secret,
            ws_url,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "T")]
enum AlpacaMessage {
    #[serde(rename = "success")]
    Success { msg: String },
    #[serde(rename = "error")]
    Error { code: i32, msg: String },
    #[serde(rename = "subscription")]
    Subscription { trades: Option<Vec<String>>, quotes: Option<Vec<String>> },
    #[serde(rename = "welcome")]
    Welcome { msg: String },
    #[serde(rename = "q")]
    Quote(AlpacaQuote),
    #[serde(rename = "t")]
    Trade(AlpacaTrade),
}

#[derive(Debug, Deserialize)]
struct AlpacaQuote {
    #[serde(rename = "S")]
    symbol: String,
    #[serde(rename = "bp")]
    bid_price: f64,
    #[serde(rename = "ap")]
    ask_price: f64,
}

#[derive(Debug, Deserialize)]
struct AlpacaTrade {
    #[serde(rename = "S")]
    symbol: String,
    #[serde(rename = "p")]
    price: f64,
}

#[async_trait]
impl MarketDataService for AlpacaMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let url = self.ws_url.clone();
        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();
        let symbols_clone = symbols.clone();

        tokio::spawn(async move {
            match connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    info!("Connected to Alpaca WebSocket");
                    let (mut write, mut read) = ws_stream.split();
                    info!("Waiting for welcome message...");
                    let mut authenticated = false;
                    let mut auth_sent = false;

                    // Read messages
                    while let Some(msg_result) = read.next().await {
                        match msg_result {
                            Ok(Message::Text(text)) => {
                                if let Ok(messages) = serde_json::from_str::<Vec<AlpacaMessage>>(&text) {
                                                                        for message in messages {
                                        match message {
                                            AlpacaMessage::Welcome { msg } => {
                                                info!("Alpaca Welcome: {}", msg);
                                            }
                                            AlpacaMessage::Success { msg } => {
                                                info!("Alpaca Success: {}", msg);
                                                
                                                // 1. If connected, send auth
                                                if msg == "connected" && !auth_sent {
                                                    let auth_msg = serde_json::json!({
                                                        "action": "auth",
                                                        "key": api_key.clone(),
                                                        "secret": api_secret.clone()
                                                    });
                                                    
                                                    if let Err(e) = write.send(Message::Text(auth_msg.to_string().into())).await {
                                                        error!("Failed to send auth message: {}", e);
                                                        return;
                                                    }
                                                    auth_sent = true;
                                                    info!("Auth message sent");
                                                }
                                                // 2. If authenticated, send subscription
                                                else if msg == "authenticated" && !authenticated {
                                                    authenticated = true;
                                                    let subscribe_msg = serde_json::json!({
                                                        "action": "subscribe",
                                                        "quotes": symbols_clone,
                                                        "trades": symbols_clone
                                                    });
                                                    
                                                    if let Err(e) = write.send(Message::Text(subscribe_msg.to_string().into())).await {
                                                        error!("Failed to send subscribe message: {}", e);
                                                        return;
                                                    }
                                                    info!("Subscription request sent for {:?}", symbols_clone);
                                                }
                                            }
                                            AlpacaMessage::Error { code, msg } => {
                                                error!("Alpaca error ({}): {}", code, msg);
                                            }
                                            AlpacaMessage::Subscription { trades, quotes } => {
                                                info!("Subscribed successfully. Trades: {:?}, Quotes: {:?}", trades, quotes);
                                            }
                                            AlpacaMessage::Quote(quote) => {
                                                // Use mid-price
                                                let mid_price = (quote.bid_price + quote.ask_price) / 2.0;
                                                let event = MarketEvent::Quote {
                                                    symbol: quote.symbol,
                                                    price: Decimal::from_f64_retain(mid_price).unwrap_or(Decimal::ZERO),
                                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                                };
                                                
                                                if tx.send(event).await.is_err() {
                                                    warn!("Market data receiver dropped");
                                                    return;
                                                }
                                            }
                                            AlpacaMessage::Trade(trade) => {
                                                let event = MarketEvent::Quote {
                                                    symbol: trade.symbol,
                                                    price: Decimal::from_f64_retain(trade.price).unwrap_or(Decimal::ZERO),
                                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                                };
                                                
                                                if tx.send(event).await.is_err() {
                                                    warn!("Market data receiver dropped");
                                                    return;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(Message::Close(_)) => {
                                info!("WebSocket closed");
                                break;
                            }
                            Err(e) => {
                                error!("WebSocket error: {}", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect to Alpaca WebSocket: {}", e);
                }
            }
        });

        Ok(rx)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        // Alpaca Data V2 Top Movers endpoint
        let url = "https://data.alpaca.markets/v2/stocks/movers";
        
        let response = self.client
            .get(url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to fetch top movers from Alpaca")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca movers fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct Mover {
            symbol: String,
        }
        #[derive(Debug, Deserialize)]
        struct MoversResponse {
            gainers: Vec<Mover>,
            // losers: Vec<Mover>,
        }

        let resp: MoversResponse = response.json().await
            .context("Failed to parse Alpaca movers response")?;

        let symbols = resp.gainers.into_iter().map(|m| m.symbol).collect();
        Ok(symbols)
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
        
        let response = self.client
            .get(url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[
                ("symbols", symbol),
                ("start", &start.to_rfc3339()),
                ("end", &end.to_rfc3339()),
                ("timeframe", timeframe),
                ("limit", "10000"), // Max limit usually
            ])
            .send()
            .await
            .context("Failed to fetch historical bars")?;
            
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca bars fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct BarsResponse {
            bars: std::collections::HashMap<String, Vec<AlpacaBar>>,
        }

        let resp: BarsResponse = response.json().await
            .context("Failed to parse Alpaca bars response")?;

        Ok(resp.bars.into_values().flatten().collect())
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
        Self {
            client: Client::new(),
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
        let tif = if is_fractional { "day" } else { "gtc" };

        let order_request = AlpacaOrderRequest {
            symbol: order.symbol.clone(),
            qty: order.quantity.to_string(),
            side: side_str.to_string(),
            order_type: "market".to_string(),
            time_in_force: tif.to_string(),
        };

        let url = format!("{}/v2/orders", self.base_url);
        
        let response = self.client
            .post(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .json(&order_request)
            .send()
            .await
            .context("Failed to send order to Alpaca")?;

        if response.status().is_success() {
            let order_resp: AlpacaOrderResponse = response.json().await
                .context("Failed to parse Alpaca order response")?;
            info!("Alpaca order placed: {} (status: {})", order_resp.id, order_resp.status);
            Ok(())
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("Alpaca order failed: {}", error_text)
        }
    }

    async fn get_portfolio(&self) -> Result<crate::domain::portfolio::Portfolio> {
        let account_url = format!("{}/v2/account", self.base_url);
        let positions_url = format!("{}/v2/positions", self.base_url);

        // Fetch Account
        let account_resp_raw = self.client
            .get(&account_url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to send account request")?;

        let account_text = account_resp_raw.text().await.context("Failed to read account response text")?;
        let account_resp: AlpacaAccount = serde_json::from_str(&account_text)
            .map_err(|e| anyhow::anyhow!("Failed to decode Alpaca Account: {}. Body: {}", e, account_text))?;

        // Fetch Positions
        let positions_resp_raw = self.client
            .get(&positions_url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to send positions request")?;

        let positions_text = positions_resp_raw.text().await.context("Failed to read positions response text")?;
        let positions_resp: Vec<AlpacaPosition> = serde_json::from_str(&positions_text)
            .map_err(|e| anyhow::anyhow!("Failed to decode Alpaca Positions: {}. Body: {}", e, positions_text))?;

        let mut portfolio = crate::domain::portfolio::Portfolio::new();
        // Use buying_power or cash? For crypto, buying_power is usually what we have available.
        // Actually, let's log both for debugging.
        let cash = account_resp.cash.parse::<Decimal>().unwrap_or(Decimal::ZERO);
        let bp = account_resp.buying_power.parse::<Decimal>().unwrap_or(Decimal::ZERO);
        
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
                average_price: alp_pos.avg_entry_price.parse::<Decimal>().unwrap_or(Decimal::ZERO),
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
        
        let response = self.client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .query(&[
                ("status", "all"),
                ("after", &today_start.to_rfc3339()),
            ])
            .send()
            .await
            .context("Failed to fetch orders from Alpaca")?;
            
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca orders fetch failed: {}", error_text);
        }
        
        let alp_orders: Vec<AlpacaOrder> = response.json().await
            .context("Failed to parse Alpaca orders")?;
            
        let mut orders = Vec::new();
        for ao in alp_orders {
            let side = if ao.side == "buy" { OrderSide::Buy } else { OrderSide::Sell };
            let qty = ao.qty.parse::<Decimal>().unwrap_or(Decimal::ZERO);
            let price = ao.filled_avg_price.as_ref()
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
                timestamp: created_at,
            });
        }
        
        Ok(orders)
    }
}
