use crate::domain::trading::types::{MarketEvent, normalize_crypto_symbol};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

pub struct BinanceWebSocketManager {
    #[allow(dead_code)]
    api_key: String,
    ws_url: String,
    spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    event_tx: broadcast::Sender<MarketEvent>,
    subscribed_symbols: Arc<RwLock<Vec<String>>>,
}

impl BinanceWebSocketManager {
    pub fn new(
        api_key: String,
        ws_url: String,
        spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let subscribed_symbols = Arc::new(RwLock::new(Vec::new()));

        Self {
            api_key,
            ws_url,
            spread_cache,
            event_tx,
            subscribed_symbols,
        }
    }

    pub async fn update_subscription(&self, symbols: Vec<String>) -> Result<()> {
        let mut subscribed = self.subscribed_symbols.write().await;
        *subscribed = symbols.clone();

        // Spawn WebSocket task if not already running
        let symbols_clone = symbols.clone();
        let ws_url = self.ws_url.clone();
        let event_tx = self.event_tx.clone();
        let spread_cache = self.spread_cache.clone();

        tokio::spawn(async move {
            Self::run_websocket(ws_url, symbols_clone, event_tx, spread_cache).await;
        });

        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MarketEvent> {
        self.event_tx.subscribe()
    }

    async fn run_websocket(
        ws_url: String,
        symbols: Vec<String>,
        event_tx: broadcast::Sender<MarketEvent>,
        spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    ) {
        let mut backoff = 1;
        const MAX_BACKOFF: u64 = 60;

        loop {
            match Self::connect_and_stream(&ws_url, &symbols, &event_tx, &spread_cache).await {
                Ok(_) => {
                    info!("Binance WebSocket connection closed gracefully");
                    backoff = 1;
                }
                Err(e) => {
                    error!(
                        "Binance WebSocket error: {}. Reconnecting in {}s...",
                        e, backoff
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
        }
    }

    async fn connect_and_stream(
        ws_url: &str,
        symbols: &[String],
        event_tx: &broadcast::Sender<MarketEvent>,
        spread_cache: &Arc<crate::application::market_data::spread_cache::SpreadCache>,
    ) -> Result<()> {
        if symbols.is_empty() {
            warn!("No symbols to subscribe to, skipping WebSocket connection");
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            return Ok(());
        }

        // Build combined stream URL
        // Format: wss://stream.binance.com:9443/stream?streams=btcusdt@trade/ethusdt@trade
        let streams: Vec<String> = symbols
            .iter()
            .map(|s| {
                let denorm = crate::domain::trading::types::denormalize_crypto_symbol(s);
                format!("{}@trade", denorm.to_lowercase())
            })
            .collect();

        let stream_param = streams.join("/");
        let url = format!("{}/stream?streams={}", ws_url, stream_param);

        info!("Connecting to Binance WebSocket: {}", url);

        let (ws_stream, _) = connect_async(&url)
            .await
            .context("Failed to connect to Binance WebSocket")?;

        info!("Binance WebSocket connected successfully");

        let (mut write, mut read) = ws_stream.split();

        // Spawn ping task
        let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(180));
        tokio::spawn(async move {
            loop {
                ping_interval.tick().await;
                if write.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        });

        // Read messages
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Err(e) = Self::handle_message(&text, event_tx, spread_cache) {
                        warn!("Failed to handle Binance message: {}", e);
                    }
                }
                Ok(Message::Ping(_)) => {
                    debug!("Received ping from Binance");
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong from Binance");
                }
                Ok(Message::Close(_)) => {
                    info!("Binance WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("Binance WebSocket read error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_message(
        text: &str,
        event_tx: &broadcast::Sender<MarketEvent>,
        _spread_cache: &Arc<crate::application::market_data::spread_cache::SpreadCache>,
    ) -> Result<()> {
        #[derive(Debug, Deserialize)]
        struct StreamMessage {
            stream: String,
            data: serde_json::Value,
        }

        let msg: StreamMessage = serde_json::from_str(text)?;

        // Handle trade stream
        if msg.stream.ends_with("@trade") {
            #[derive(Debug, Deserialize)]
            struct TradeData {
                #[serde(rename = "s")]
                symbol: String,
                #[serde(rename = "p")]
                price: String,
                #[serde(rename = "q")]
                quantity: String,
                #[serde(rename = "T")]
                trade_time: i64,
            }

            let trade: TradeData = serde_json::from_value(msg.data)?;

            // Normalize symbol
            let normalized_symbol =
                normalize_crypto_symbol(&trade.symbol).unwrap_or_else(|_| trade.symbol.clone());

            let price = trade
                .price
                .parse::<f64>()
                .ok()
                .and_then(Decimal::from_f64_retain)
                .unwrap_or(Decimal::ZERO);

            let _quantity = trade
                .quantity
                .parse::<f64>()
                .ok()
                .and_then(Decimal::from_f64_retain)
                .unwrap_or(Decimal::ZERO);

            let event = MarketEvent::Quote {
                symbol: normalized_symbol,
                price,
                timestamp: trade.trade_time,
            };

            let _ = event_tx.send(event);
        }

        Ok(())
    }
}
