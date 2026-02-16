use crate::domain::ports::OrderUpdate;
use crate::domain::trading::types::{OrderSide, OrderStatus};
use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio::time::{self, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{error, info, warn};

// Constants for connection management
const PING_INTERVAL_SECS: u64 = 20;

const MAX_RECONNECT_DELAY_SECS: u64 = 30;

/// Connection state for the Trading Stream
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Disconnected,
    Connected,
    Authenticated,
    Subscribed,
}

/// Manager for Alpaca Trading Stream (Account Updates)
pub struct AlpacaTradingStream {
    api_key: String,
    api_secret: String,
    base_url: String, // e.g. https://paper-api.alpaca.markets -> wss://paper-api.alpaca.markets/stream
    event_tx: broadcast::Sender<OrderUpdate>,
    state: Arc<RwLock<ConnectionState>>,
}

// Alpaca Stream Messages
#[derive(Debug, Deserialize)]
#[serde(tag = "stream")]
enum StreamMessage {
    #[serde(rename = "authorization")]
    Authorization { data: AuthData },
    #[serde(rename = "listening")]
    Listening { data: ListeningData },
    #[serde(rename = "trade_updates")]
    TradeUpdate { data: TradeUpdateData },
}

#[derive(Debug, Deserialize)]

struct AuthData {
    status: String,
}

#[derive(Debug, Deserialize)]
struct ListeningData {
    streams: Vec<String>,
}

#[derive(Debug, Deserialize)]

struct TradeUpdateData {
    order: AlpacaOrderData,
}

#[derive(Debug, Deserialize)]
struct AlpacaOrderData {
    id: String,
    client_order_id: String,
    symbol: String,
    side: String,
    #[serde(default)]
    filled_qty: String,
    #[serde(default)]
    filled_avg_price: Option<String>,
    status: String,
}

impl AlpacaTradingStream {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        let (event_tx, _) = broadcast::channel(100);

        let stream = Self {
            api_key,
            api_secret,
            base_url,
            event_tx,
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
        };

        stream.spawn_connection_task();
        stream
    }

    pub fn subscribe(&self) -> broadcast::Receiver<OrderUpdate> {
        self.event_tx.subscribe()
    }

    fn spawn_connection_task(&self) {
        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();

        // Convert HTTP Base URL to WebSocket Stream URL
        // https://paper-api.alpaca.markets -> wss://paper-api.alpaca.markets/stream
        let ws_url = self
            .base_url
            .replace("https://", "wss://")
            .replace("http://", "ws://")
            + "/stream";

        let event_tx = self.event_tx.clone();
        let state = self.state.clone();

        tokio::spawn(async move {
            let mut reconnect_attempts = 0;

            loop {
                info!("TradingStream: Connecting to {}...", ws_url);

                match Self::run_connection(&ws_url, &api_key, &api_secret, &event_tx, &state).await
                {
                    Ok(_) => {
                        info!("TradingStream: Connection closed cleanly");
                        reconnect_attempts = 0;
                    }
                    Err(e) => {
                        error!("TradingStream error: {}. Reconnecting...", e);
                        *state.write().await = ConnectionState::Disconnected;

                        // Exponential backoff
                        let delay =
                            std::cmp::min(2u64.pow(reconnect_attempts), MAX_RECONNECT_DELAY_SECS);
                        time::sleep(Duration::from_secs(delay)).await;
                        reconnect_attempts += 1;
                    }
                }
            }
        });
    }

    async fn run_connection(
        url: &str,
        key: &str,
        secret: &str,
        tx: &broadcast::Sender<OrderUpdate>,
        state: &Arc<RwLock<ConnectionState>>,
    ) -> Result<()> {
        let (ws_stream, _) = connect_async(url).await.context("Failed to connect")?;
        info!("TradingStream: Connected");
        *state.write().await = ConnectionState::Connected;

        let (mut write, mut read) = ws_stream.split();

        // 1. Authenticate
        let auth_msg = serde_json::json!({
            "action": "authenticate",
            "data": {
                "key_id": key,
                "secret_key": secret
            }
        });
        write
            .send(Message::Text(auth_msg.to_string().into()))
            .await?;
        info!("TradingStream: Sent authentication");

        // Heartbeat
        let mut ping_interval = time::interval(Duration::from_secs(PING_INTERVAL_SECS));

        loop {
            tokio::select! {
                Some(msg) = read.next() => {
                    match msg? {
                        Message::Text(text) => {
                            // Decode message
                            if let Ok(stream_msg) = serde_json::from_str::<StreamMessage>(&text) {
                                match stream_msg {
                                    StreamMessage::Authorization { data } => {
                                        if data.status == "authorized" {
                                            info!("TradingStream: Authenticated successfully");
                                            *state.write().await = ConnectionState::Authenticated;

                                            // 2. Subscribe to trade_updates
                                            let sub_msg = serde_json::json!({
                                                "action": "listen",
                                                "data": {
                                                    "streams": ["trade_updates"]
                                                }
                                            });
                                            write.send(Message::Text(sub_msg.to_string().into())).await?;
                                        } else {
                                            return Err(anyhow::anyhow!("Authentication failed: {}", data.status));
                                        }
                                    },
                                    StreamMessage::Listening { data } => {
                                        info!("TradingStream: Subscribed to {:?}", data.streams);
                                        *state.write().await = ConnectionState::Subscribed;
                                    },
                                    StreamMessage::TradeUpdate { data } => {
                                        Self::handle_trade_update(data, tx);
                                    }
                                }
                            } else {
                                // Sometimes messages are arrays or other formats?
                                // Alpaca Stream usually sends single objects for these events
                                warn!("TradingStream: Unhandled message format: {}", text);
                            }
                        },
                        Message::Ping(_) => {
                            write.send(Message::Pong(vec![].into())).await?;
                        },
                        Message::Close(_) => return Ok(()),
                        _ => {}
                    }
                }
                _ = ping_interval.tick() => {
                    write.send(Message::Ping(vec![].into())).await?;
                }
            }
        }
    }

    fn handle_trade_update(data: TradeUpdateData, tx: &broadcast::Sender<OrderUpdate>) {
        // Map Alpaca status to Domain OrderStatus
        let status = match data.order.status.as_str() {
            "new" => OrderStatus::Pending,
            "filled" => OrderStatus::Filled,
            "partially_filled" => OrderStatus::PartiallyFilled,
            "canceled" => OrderStatus::Canceled,
            "rejected" => OrderStatus::Rejected,
            _ => OrderStatus::Pending,
        };

        // If event is specifically "fill" or "partial_fill", we trust that.
        // But the status field is usually enough.

        let filled_qty = data
            .order
            .filled_qty
            .parse::<Decimal>()
            .unwrap_or(Decimal::ZERO);
        let filled_avg_price = data
            .order
            .filled_avg_price
            .and_then(|p| p.parse::<Decimal>().ok());

        let side = match data.order.side.as_str() {
            "buy" => OrderSide::Buy,
            "sell" => OrderSide::Sell,
            _ => OrderSide::Buy, // Safe default?
        };

        let event = OrderUpdate {
            order_id: data.order.id,
            client_order_id: data.order.client_order_id,
            symbol: data.order.symbol,
            side,
            status,
            filled_qty,
            filled_avg_price,
            timestamp: Utc::now(), // Ideally parse data.timestamp
        };

        if let Err(e) = tx.send(event) {
            warn!("TradingStream: Failed to broadcast update: {}", e);
        }
    }
}
