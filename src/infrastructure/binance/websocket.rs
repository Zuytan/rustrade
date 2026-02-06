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
    // Handle for the active WebSocket task to allow cancellation
    task_handle: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl BinanceWebSocketManager {
    pub fn new(
        api_key: String,
        ws_url: String,
        spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let subscribed_symbols = Arc::new(RwLock::new(Vec::new()));
        let task_handle = Arc::new(tokio::sync::Mutex::new(None));

        Self {
            api_key,
            ws_url,
            spread_cache,
            event_tx,
            subscribed_symbols,
            task_handle,
        }
    }

    pub async fn update_subscription(&self, symbols: Vec<String>) -> Result<()> {
        let mut subscribed = self.subscribed_symbols.write().await;
        *subscribed = symbols.clone();

        // Cancel existing task if running
        let mut handle_guard = self.task_handle.lock().await;
        if let Some(handle) = handle_guard.take() {
            debug!("BinanceWebSocketManager: Aborting previous WebSocket task");
            handle.abort();
        }

        // Validate symbol list is not empty before spawning
        if symbols.is_empty() {
            info!("BinanceWebSocketManager: Subscription empty, not spawning task");
            return Ok(());
        }

        // Spawn new WebSocket task
        let symbols_clone = symbols.clone();
        let ws_url = self.ws_url.clone();
        let event_tx = self.event_tx.clone();
        let spread_cache = self.spread_cache.clone();

        let handle = tokio::spawn(async move {
            Self::run_websocket(ws_url, symbols_clone, event_tx, spread_cache).await;
        });

        *handle_guard = Some(handle);
        info!(
            "BinanceWebSocketManager: Spawned new WebSocket task for {} symbols",
            symbols.len()
        );

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
                    // Prevent rapid reconnect loop if server closes connection repeatedly
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
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

        // Use /stream endpoint for combined streams via JSON subscription
        // Note: Testnet requires /stream, Mainnet /stream
        // Use /stream endpoint with query params for small lists to improve stability
        // Large lists must use JSON-RPC subscription to avoid URL length limits
        let use_url_params = symbols.len() < 50;

        let all_streams: Vec<String> = symbols
            .iter()
            .map(|s| {
                let denorm = crate::domain::trading::types::denormalize_crypto_symbol(s);
                format!("{}@trade", denorm.to_lowercase())
            })
            .collect();

        let mut url = format!("{}/stream", ws_url.trim_end_matches('/'));

        if use_url_params {
            let query = all_streams.join("/");
            if !query.is_empty() {
                url.push_str("?streams=");
                url.push_str(&query);
            }
        }

        info!("Connecting to Binance WebSocket: {}", url);

        let (ws_stream, _) = connect_async(&url)
            .await
            .context("Failed to connect to Binance WebSocket")?;

        info!("Binance WebSocket connected successfully");

        let (mut write, mut read) = ws_stream.split();

        // If we didn't use URL params, we must subscribe via JSON
        if !use_url_params {
            const BATCH_SIZE: usize = 10; // Reduced for Testnet stability
            for chunk in all_streams.chunks(BATCH_SIZE) {
                let subscribe_msg = serde_json::json!({
                    "method": "SUBSCRIBE",
                    "params": chunk,
                    "id": chrono::Utc::now().timestamp_millis()
                });

                let msg_str = subscribe_msg.to_string();
                debug!("Sending subscription batch: {} streams", chunk.len());

                if let Err(e) = write.send(Message::Text(msg_str.into())).await {
                    error!("Failed to send subscription message: {}", e);
                    return Err(anyhow::anyhow!("Failed to subscribe to streams"));
                }

                // Increased delay to respect rate limits (5 commands/s max usually)
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }

        // Spawn ping task
        let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(180));
        let mut write_clone = write; // Move ownership to task

        // We need a way to keep 'write' alive for pings AND allow strictly read loop?
        // Actually, split() gives us independent sink/stream.
        // But we just moved 'write' into the loop above? No we used 'write' to send subscribes.
        // Now we move it to ping task.

        // Wait! write is Sink. We need it to be Arc Mutex or channel controlled if we want to write from multiple places?
        // But here we only write Pings after subscription.

        // ISSUE: connect_async returns a Stream that implements Stream + Sink.
        // split() returns SplitSink and SplitStream.
        // We typically spawn a writer task that handles Pings using the Sink.

        // Let's restructure to use a channel for outgoing messages so we can send Pings and Subscribes.
        let (ws_tx, mut ws_rx) = tokio::sync::mpsc::channel::<Message>(100);

        tokio::spawn(async move {
            while let Some(msg) = ws_rx.recv().await {
                if write_clone.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Spawn Ping Generator
        let tx_ping = ws_tx.clone();
        tokio::spawn(async move {
            loop {
                ping_interval.tick().await;
                if tx_ping.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        });

        // Read messages
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Err(e) = Self::handle_message(&text, event_tx, spread_cache) {
                        // Don't error on "result": null (subscription response)
                        if !text.contains("\"result\":null") {
                            warn!("Failed to handle Binance message: {}", e);
                        }
                    }
                }
                Ok(Message::Ping(_)) => {
                    debug!("Received ping from Binance");
                    let _ = ws_tx.send(Message::Pong(vec![].into())).await;
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong from Binance");
                }
                Ok(Message::Close(frame)) => {
                    if let Some(cf) = frame {
                        info!(
                            "Binance WebSocket closed by server: Code {} Reason '{}'",
                            cf.code, cf.reason
                        );
                    } else {
                        info!("Binance WebSocket closed by server (No info)");
                    }
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

            let quantity = trade
                .quantity
                .parse::<f64>()
                .ok()
                .and_then(Decimal::from_f64_retain)
                .unwrap_or(Decimal::ZERO);

            let event = MarketEvent::Quote {
                symbol: normalized_symbol,
                price,
                quantity,
                timestamp: trade.trade_time,
            };

            let _ = event_tx.send(event);
        }

        Ok(())
    }
}
