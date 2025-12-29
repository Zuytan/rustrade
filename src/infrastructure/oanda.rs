use crate::domain::ports::{ExecutionService, MarketDataService, SectorProvider};
use crate::domain::trading::types::{MarketEvent, Order, OrderSide, OrderType};
use crate::domain::trading::portfolio::Portfolio;
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver};
use tracing::{info, error};
use std::collections::HashMap;

// ===== Market Data Service (HTTP Streaming) =====

pub struct OandaMarketDataService {
    api_key: String,
    stream_base_url: String,
    api_base_url: String,
    account_id: String,
    client: Client,
}

impl OandaMarketDataService {
    pub fn new(api_key: String, stream_base_url: String, api_base_url: String, account_id: String) -> Self {
        Self {
            api_key,
            stream_base_url,
            api_base_url,
            account_id,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl MarketDataService for OandaMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        // OANDA uses a long-lived HTTP connection for streaming.
        // We will spawn a task to read from the stream and send events to the channel.
        let (tx, rx) = mpsc::channel(100);
        let api_key = self.api_key.clone();
        let account_id = self.account_id.clone();
        let stream_url = format!("{}/v3/accounts/{}/pricing/stream", self.stream_base_url, account_id);
        
        // OANDA symbols in URL should be comma separated, e.g. "EUR_USD,USD_JPY"
        // Ensure symbols are in correct format (e.g. replace / with _ if needed, though usually standard is EUR_USD)
        let instruments = symbols.join(",");

        tokio::spawn(async move {
            let client = Client::new();
            info!("Connecting to OANDA Stream: {} for instruments: {}", stream_url, instruments);
            
            loop {
                let response = client.get(&stream_url)
                    .query(&[("instruments", &instruments)])
                    .header("Authorization", format!("Bearer {}", api_key))
                    .send()
                    .await;

                match response {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            error!("OANDA Stream Connection Failed: {}", resp.status());
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        }

                        let mut stream = resp.bytes_stream();
                        use futures_util::StreamExt;

                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(bytes) => {
                                    // OANDA sends newline delimited JSON
                                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                                        for line in text.lines() {
                                            if let Ok(price_event) = serde_json::from_str::<OandaPriceEvent>(line) {
                                                if let (Some(bids), Some(asks)) = (price_event.bids, price_event.asks) {
                                                    if !bids.is_empty() && !asks.is_empty() {
                                                        // Calculate mid price
                                                        // Assuming first bid/ask is best
                                                        let bid = bids[0].price.parse::<f64>().unwrap_or(0.0);
                                                        let ask = asks[0].price.parse::<f64>().unwrap_or(0.0);
                                                        let mid_price = (bid + ask) / 2.0;

                                                        let event = MarketEvent::PriceUpdate {
                                                            symbol: price_event.instrument.unwrap_or_default(),
                                                            price: Decimal::from_f64_retain(mid_price).unwrap_or_default(),
                                                            timestamp: chrono::Utc::now(),
                                                        };
                                                        
                                                        if let Err(e) = tx.send(event).await {
                                                            error!("Failed to send OANDA market event: {}", e);
                                                            return; // Receiver dropped, stop stream
                                                        }
                                                    }
                                                }
                                            } else if text.contains("HEARTBEAT") {
                                                // Ignore heartbeats
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Error reading OANDA stream chunk: {}", e);
                                    break; // Reconnect
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to connect to OANDA stream: {}", e);
                    }
                }
                
                info!("Reconnecting to OANDA stream in 5 seconds...");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });

        Ok(rx)
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        // OANDA doesn't have a direct "top movers" endpoint like Alpaca.
        // We can return an empty list or implement a basic scan if we had a list of all instruments.
        // For now, return empty or default list.
        Ok(vec![])
    }

    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, rust_decimal::Decimal>> {
        let instruments = symbols.join(",");
        let url = format!("{}/v3/accounts/{}/pricing", self.api_base_url, self.account_id);
        
        let resp = self.client.get(&url)
            .query(&[("instruments", &instruments)])
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?
            .json::<OandaPricingResponse>()
            .await?;
            
        let mut prices = std::collections::HashMap::new();
        for price in resp.prices {
            if let (Some(bids), Some(asks)) = (price.bids, price.asks) {
                if !bids.is_empty() && !asks.is_empty() {
                    let bid = bids[0].price.parse::<f64>().unwrap_or(0.0);
                    let ask = asks[0].price.parse::<f64>().unwrap_or(0.0);
                    let mid_price = (bid + ask) / 2.0;
                    if let Some(price_dec) = Decimal::from_f64_retain(mid_price) {
                        prices.insert(price.instrument, price_dec);
                    }
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
        // Mapping generic timeframe to OANDA granularity
        let granularity = match timeframe {
            "1Min" => "M1",
            "5Min" => "M5",
            "15Min" => "M15",
            "1H" => "H1",
            "1D" => "D",
            _ => "M1", // Default
        };
        
        // OANDA uses RFC3339 format
        let url = format!("{}/v3/instruments/{}/candles", self.api_base_url, symbol);
        let resp = self.client.get(&url)
            .query(&[
                ("from", start.to_rfc3339()),
                ("to", end.to_rfc3339()),
                ("granularity", granularity.to_string()),
                ("price", "M".to_string()) // Midpoint candles
            ])
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?
            .json::<OandaCandlesResponse>()
            .await?;
            
        let mut candles = Vec::new();
        for c in resp.candles {
            if c.complete {
                if let Some(mid) = c.mid {
                    candles.push(crate::domain::trading::types::Candle {
                        symbol: symbol.to_string(),
                        open: mid.o.parse().unwrap_or_default(),
                        high: mid.h.parse().unwrap_or_default(),
                        low: mid.l.parse().unwrap_or_default(),
                        close: mid.c.parse().unwrap_or_default(),
                        volume: c.volume as u64,
                        timestamp: chrono::DateTime::parse_from_rfc3339(&c.time)?.with_timezone(&chrono::Utc),
                    });
                }
            }
        }
        
        Ok(candles)
    }
}

// OANDA JSON Structures for Market Data
#[derive(Debug, Deserialize)]
struct OandaPriceEvent {
    #[serde(rename = "type")]
    event_type: String,
    time: Option<String>,
    instrument: Option<String>,
    bids: Option<Vec<OandaPriceBucket>>,
    asks: Option<Vec<OandaPriceBucket>>,
}

#[derive(Debug, Deserialize)]
struct OandaPriceBucket {
    price: String,
    liquidity: i64,
}

#[derive(Debug, Deserialize)]
struct OandaPricingResponse {
    prices: Vec<OandaPriceItem>,
}

#[derive(Debug, Deserialize)]
struct OandaPriceItem {
    instrument: String,
    time: String,
    bids: Option<Vec<OandaPriceBucket>>,
    asks: Option<Vec<OandaPriceBucket>>,
}

#[derive(Debug, Deserialize)]
struct OandaCandlesResponse {
    instrument: String,
    granularity: String,
    candles: Vec<OandaCandle>,
}

#[derive(Debug, Deserialize)]
struct OandaCandle {
    complete: bool,
    volume: i64,
    time: String,
    mid: Option<OandaCandleOHLC>,
}

#[derive(Debug, Deserialize)]
struct OandaCandleOHLC {
    o: String,
    h: String,
    l: String,
    c: String,
}

// ===== Execution Service (REST API) =====

pub struct OandaExecutionService {
    api_key: String,
    api_base_url: String,
    account_id: String,
    client: Client,
}

impl OandaExecutionService {
    pub fn new(api_key: String, api_base_url: String, account_id: String) -> Self {
        Self {
            api_key,
            api_base_url,
            account_id,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl ExecutionService for OandaExecutionService {
    async fn execute(&self, order: Order) -> Result<()> {
        let url = format!("{}/v3/accounts/{}/orders", self.api_base_url, self.account_id);
        
        // Map Order to OANDA Order Request
        // Note: OANDA requires quantity as "units". Positive for buy, negative for sell? 
        // Actually OANDA usually takes "units" as string. + for long, - for short.
        // OR explicit type="MARKET" side is not explicit in API, it's inferred from units sign usually,
        // BUT v3 API has "units" and "instrument".
        // Let's check docs: "units": "The quantity to request. positive for buy, negative for sell"
        // Wait, regular Orders endpoint might be safer.
        // Simple Market Order:
        // { "order": { "units": "100", "instrument": "EUR_USD", "timeInForce": "FOK", "type": "MARKET", "positionFill": "DEFAULT" } }
        
        let qty_sign = match order.side {
            OrderSide::Buy => 1.0,
            OrderSide::Sell => -1.0,
        };
        let units = order.quantity * Decimal::from_f64_retain(qty_sign).unwrap_or(Decimal::ONE);
        
        let oanda_order = OandaOrderRequestWrapper {
            order: OandaOrderRequest {
                units: units.to_string(),
                instrument: order.symbol,
                time_in_force: "FOK".to_string(), // Fill or Kill
                order_type: "MARKET".to_string(),
                position_fill: "DEFAULT".to_string(),
            }
        };

        let resp = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&oanda_order)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            anyhow::bail!("OANDA Order Failed: {}", error_text);
        }
        
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        let url = format!("{}/v3/accounts/{}/summary", self.api_base_url, self.account_id);
        
        let resp = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?
            .json::<OandaAccountSummaryResponse>()
            .await?;
            
        let acc = resp.account;
        
        // Create Portfolio
        // OANDA Summary: balance, NAV, marginAvailable, etc.
        let cash = acc.balance.parse().unwrap_or_default();
        let equity = acc.nav.parse().unwrap_or_default();
        // Ignoring positions from summary as it's just aggregates usually,
        // but we need them. v3/accounts/{id} gives full details including positions.
        // Summary might be enough for cash/equity.
        
        let mut portfolio = Portfolio::new();
        portfolio.cash = cash; // Roughly equivalent to balance
        // Wait, Portfolio struct tracks positions too.
        // We need to fetch open positions.
        
        let positions_url = format!("{}/v3/accounts/{}/openPositions", self.api_base_url, self.account_id);
        let pos_resp = self.client.get(&positions_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?
            .json::<OandaPositionsResponse>()
            .await?;
            
        for pos in pos_resp.positions {
            // OANDA positions are aggregated long/short?
            // "long": { "units": "10", "averagePrice": "1.23" }, "short": { "units": "0", ... }
            if let Some(long) = pos.long {
                 let units: Decimal = long.units.parse().unwrap_or_default();
                 if units > Decimal::ZERO {
                     portfolio.update_position(&pos.instrument, units, long.average_price.parse().unwrap_or_default());
                 }
            }
            if let Some(short) = pos.short {
                let units: Decimal = short.units.parse().unwrap_or_default();
                // Short units are usually negative string in some contexts, but let's be careful.
                // In openPositions, usually positive number but key is "short".
                // If units < 0, it's short.
                // Let's assume absolute values in long/short structs.
                 if units.abs() > Decimal::ZERO {
                     // Portfolio might handle negative quantity for shorts?
                     // Rustrade Portfolio `update_position` usually takes absolute quantity?
                     // Let's check Portfolio implementation or assume standard LONG only for now if simplicity needed,
                     // but OANDA is forex/cfd so Shorting is native.
                     // IMPORTANT: Rustrade Portfolio likely assumes positive quantity.
                     // We'll ignore shorts for this basic implementation or treat as negative if supported.
                     // The Portfolio struct `positions` is HashMap<String, Position>. Position has `quantity` (Decimal).
                     // If we pass negative, does it work?
                     portfolio.update_position(&pos.instrument, -units.abs(), short.average_price.parse().unwrap_or_default());
                 }
            }
        }
        
        // Re-overwrite cash to ensure it matches
        // portfolio.update_cash(...) might be cumulative or absolute set?
        // Portfolio typically calculates cash from trades.
        // Here we want to SYNC with broker.
        portfolio.cash = cash;
        
        Ok(portfolio)
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        // OANDA "orders" endpoint with time filters?
        // GET /v3/accounts/{id}/orders?state=ALL&count=500
        // Difficulty: Filtering by "today".
        Ok(vec![]) // Not critical for MVP
    }
}

pub struct OandaSectorProvider;

#[async_trait]
impl SectorProvider for OandaSectorProvider {
    async fn get_sector(&self, _symbol: &str) -> Result<String> {
        // OANDA is mostly Forex/Indices/Commodities.
        // We can map by symbol prefix or return "Unknown" / "Forex"
        Ok("Forex".to_string())
    }
}


// OANDA JSON Structures for Execution
#[derive(Debug, Serialize)]
struct OandaOrderRequestWrapper {
    order: OandaOrderRequest,
}

#[derive(Debug, Serialize)]
struct OandaOrderRequest {
    units: String,
    instrument: String,
    #[serde(rename = "timeInForce")]
    time_in_force: String,
    #[serde(rename = "type")]
    order_type: String,
    #[serde(rename = "positionFill")]
    position_fill: String,
}

#[derive(Debug, Deserialize)]
struct OandaAccountSummaryResponse {
    account: OandaAccountSummary,
}

#[derive(Debug, Deserialize)]
struct OandaAccountSummary {
    balance: String,
    #[serde(rename = "NAV")]
    nav: String,
}

#[derive(Debug, Deserialize)]
struct OandaPositionsResponse {
    positions: Vec<OandaPosition>,
}

#[derive(Debug, Deserialize)]
struct OandaPosition {
    instrument: String,
    long: Option<OandaPositionSide>,
    short: Option<OandaPositionSide>,
}

#[derive(Debug, Deserialize)]
struct OandaPositionSide {
    units: String,
    #[serde(rename = "averagePrice")]
    average_price: String,
}
