use crate::domain::trading::types::{MarketEvent, normalize_crypto_symbol};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// A connection silent for this long is considered dead and is reopened. Binance sends a
/// ping frame every 20 seconds, so a healthy connection is never idle this long.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Aborts the wrapped tasks when dropped.
struct AbortOnDrop(Vec<tokio::task::JoinHandle<()>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        for handle in &self.0 {
            handle.abort();
        }
    }
}

pub struct BinanceWebSocketManager {
    ws_url: String,
    event_tx: broadcast::Sender<MarketEvent>,
    subscribed_symbols: Arc<RwLock<Vec<String>>>,
    // Handle for the active WebSocket task to allow cancellation
    task_handle: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl BinanceWebSocketManager {
    pub fn new(ws_url: String) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let subscribed_symbols = Arc::new(RwLock::new(Vec::new()));
        let task_handle = Arc::new(tokio::sync::Mutex::new(None));

        Self {
            ws_url,
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
        let handle = tokio::spawn(async move {
            Self::run_websocket(ws_url, symbols_clone, event_tx).await;
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
    ) {
        let mut backoff = 1;
        const MAX_BACKOFF: u64 = 60;

        loop {
            match Self::connect_and_stream(&ws_url, &symbols, &event_tx, IDLE_TIMEOUT).await {
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
        idle_timeout: Duration,
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

        let writer_task = tokio::spawn(async move {
            while let Some(msg) = ws_rx.recv().await {
                if write_clone.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Spawn Ping Generator
        let tx_ping = ws_tx.clone();
        let ping_task = tokio::spawn(async move {
            loop {
                ping_interval.tick().await;
                if tx_ping.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        });

        // The helper tasks belong to this connection only: stop them on any exit path.
        let _abort_helpers = AbortOnDrop(vec![writer_task, ping_task]);

        // Read messages. Any frame (data, ping, pong) proves the connection is alive;
        // silence beyond `idle_timeout` means a half-open connection, so reconnect.
        loop {
            let msg_result = match tokio::time::timeout(idle_timeout, read.next()).await {
                Ok(Some(msg_result)) => msg_result,
                Ok(None) => break,
                Err(_) => {
                    anyhow::bail!(
                        "No message from Binance WebSocket for {:?}, reconnecting",
                        idle_timeout
                    );
                }
            };
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Err(e) = Self::handle_message(&text, event_tx) {
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
                    // An abrupt drop is an error (backoff reconnect), not a graceful close.
                    return Err(anyhow::anyhow!("Binance WebSocket read error: {}", e));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_message(text: &str, event_tx: &broadcast::Sender<MarketEvent>) -> Result<()> {
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

            // Parsed exactly as Decimal. A malformed or non-positive price is dropped:
            // it must never reach the candle pipeline as a zero quote.
            let price = Decimal::from_str(&trade.price)
                .with_context(|| format!("Invalid trade price '{}'", trade.price))?;
            if price <= Decimal::ZERO {
                anyhow::bail!("Non-positive trade price {} for {}", price, trade.symbol);
            }
            let quantity = Decimal::from_str(&trade.quantity)
                .with_context(|| format!("Invalid trade quantity '{}'", trade.quantity))?;
            if quantity < Decimal::ZERO {
                anyhow::bail!("Negative trade quantity {} for {}", quantity, trade.symbol);
            }

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

#[cfg(test)]
mod chaos_tests {
    //! Chaos tests against a local WebSocket server: malformed, duplicated and
    //! out-of-order messages, abrupt disconnects and silent (half-open) connections.

    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    fn trade(symbol: &str, price: &str, time: i64) -> String {
        serde_json::json!({
            "stream": format!("{}@trade", symbol.to_lowercase()),
            "data": { "s": symbol, "p": price, "q": "0.5", "T": time }
        })
        .to_string()
    }

    async fn listener() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}", listener.local_addr().expect("addr"));
        (listener, url)
    }

    fn symbols() -> Vec<String> {
        vec!["BTC/USDT".to_string()]
    }

    async fn next_quote(rx: &mut broadcast::Receiver<MarketEvent>) -> (Decimal, i64) {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("quote within 5s")
            .expect("channel open");
        match event {
            MarketEvent::Quote {
                price, timestamp, ..
            } => (price, timestamp),
            other => panic!("unexpected event {:?}", other),
        }
    }

    #[test]
    fn test_malformed_prices_are_dropped_not_zeroed() {
        let (tx, mut rx) = broadcast::channel(10);
        for bad in ["abc", "", "0", "-5", "NaN"] {
            assert!(
                BinanceWebSocketManager::handle_message(&trade("BTCUSDT", bad, 1), &tx).is_err(),
                "price {:?} must be rejected",
                bad
            );
        }
        assert!(rx.try_recv().is_err(), "no event may be emitted");

        BinanceWebSocketManager::handle_message(&trade("BTCUSDT", "65000.12", 1), &tx)
            .expect("valid trade");
        match rx.try_recv().expect("valid quote") {
            MarketEvent::Quote { price, .. } => {
                assert_eq!(price, Decimal::from_str("65000.12").expect("dec"))
            }
            other => panic!("unexpected event {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_stream_survives_garbage_duplicates_and_reordering() {
        let (listener, url) = listener().await;
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut ws = accept_async(tcp).await.expect("handshake");
            for msg in [
                trade("BTCUSDT", "100", 3),
                "not json at all".to_string(),
                r#"{"result":null,"id":1}"#.to_string(),
                trade("BTCUSDT", "garbage", 4),
                trade("BTCUSDT", "100", 3), // duplicate
                trade("BTCUSDT", "99", 1),  // out of order
            ] {
                ws.send(Message::Text(msg.into())).await.expect("send");
            }
            ws.close(None).await.expect("close");
        });

        let (tx, mut rx) = broadcast::channel(10);
        let result = BinanceWebSocketManager::connect_and_stream(
            &url,
            &symbols(),
            &tx,
            Duration::from_secs(5),
        )
        .await;
        assert!(result.is_ok(), "graceful close: {:?}", result);

        // Valid frames are delivered as received; invalid ones are skipped without
        // breaking the stream. Deduplication and ordering belong to the candle pipeline.
        assert_eq!(next_quote(&mut rx).await, (Decimal::from(100), 3));
        assert_eq!(next_quote(&mut rx).await, (Decimal::from(100), 3));
        assert_eq!(next_quote(&mut rx).await, (Decimal::from(99), 1));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_silent_connection_times_out() {
        let (listener, url) = listener().await;
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let _ws = accept_async(tcp).await.expect("handshake");
            // Half-open: keep the socket but never send anything.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        let (tx, _rx) = broadcast::channel(10);
        let started = std::time::Instant::now();
        let result = BinanceWebSocketManager::connect_and_stream(
            &url,
            &symbols(),
            &tx,
            Duration::from_millis(300),
        )
        .await;

        assert!(result.is_err(), "a silent connection must not hang");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_reconnects_after_abrupt_disconnect() {
        let (listener, url) = listener().await;
        tokio::spawn(async move {
            // First connection: one trade, then the socket is dropped without a close frame.
            let (tcp, _) = listener.accept().await.expect("accept 1");
            let mut ws = accept_async(tcp).await.expect("handshake 1");
            ws.send(Message::Text(trade("BTCUSDT", "100", 1).into()))
                .await
                .expect("send 1");
            drop(ws);

            // Second connection: the client must come back on its own.
            let (tcp, _) = listener.accept().await.expect("accept 2");
            let mut ws = accept_async(tcp).await.expect("handshake 2");
            ws.send(Message::Text(trade("BTCUSDT", "101", 2).into()))
                .await
                .expect("send 2");
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let (tx, mut rx) = broadcast::channel(10);
        let task = tokio::spawn(BinanceWebSocketManager::run_websocket(url, symbols(), tx));

        assert_eq!(next_quote(&mut rx).await, (Decimal::from(100), 1));
        assert_eq!(next_quote(&mut rx).await, (Decimal::from(101), 2));
        task.abort();
    }
}
