//! WebSocket Connection Manager for Alpaca
//!
//! This module implements a singleton WebSocket connection manager that maintains
//! a single persistent connection to Alpaca's market data stream. This prevents
//! "connection limit exceeded" errors by reusing the same connection and updating
//! subscriptions dynamically via WebSocket messages.
//!
//! # Architecture
//!
//! - **Singleton Pattern**: One WebSocket connection per AlpacaMarketDataService instance
//! - **Observer Pattern**: Broadcast channel allows multiple subscribers
//! - **Command Pattern**: Update subscriptions via command channel without reconnecting
//!
//! # Example
//!
//! ```rust,no_run
//! use rustrade::infrastructure::alpaca_websocket::AlpacaWebSocketManager;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let api_key = "key".to_string();
//! let api_secret = "secret".to_string();
//! let ws_url = "wss://example.com".to_string();
//!
//! let manager = AlpacaWebSocketManager::new(api_key, api_secret, ws_url);
//!
//! // Subscribe to events
//! let mut rx = manager.subscribe();
//!
//! // Update symbols dynamically
//! manager.update_subscription(vec!["AAPL".to_string()]).await?;
//! # Ok(())
//! # }
//! ```

use crate::domain::trading::types::MarketEvent;
use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::{self, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error, info, warn};

// WebSocket heartbeat and reconnection configuration
const PING_INTERVAL_SECS: u64 = 20;
const PONG_TIMEOUT_SECS: u64 = 5;
const MAX_RECONNECT_DELAY_SECS: u64 = 30;

/// Connection state for the WebSocket
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connected,
    Authenticated,
    Subscribed,
}

/// Commands that can be sent to the WebSocket task
#[derive(Debug)]
enum SubscriptionCommand {
    UpdateSymbols(Vec<String>),
    Shutdown,
}

/// Persistent WebSocket connection manager
pub struct AlpacaWebSocketManager {
    /// WebSocket URL
    ws_url: String,

    /// API credentials
    api_key: String,
    api_secret: String,

    /// Broadcast sender for market events (multiple receivers can subscribe)
    event_tx: broadcast::Sender<MarketEvent>,

    /// Currently subscribed symbols
    subscribed_symbols: Arc<RwLock<Vec<String>>>,

    /// Command channel to update subscriptions
    command_tx: mpsc::Sender<SubscriptionCommand>,

    /// Current connection state
    state: Arc<RwLock<ConnectionState>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "T")]
enum AlpacaMessage {
    #[serde(rename = "success")]
    Success { msg: String },
    #[serde(rename = "error")]
    Error { code: i32, msg: String },
    #[serde(rename = "subscription")]
    Subscription {
        trades: Option<Vec<String>>,
        quotes: Option<Vec<String>>,
    },
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

impl AlpacaWebSocketManager {
    /// Create a new WebSocket manager and start the background connection task
    pub fn new(api_key: String, api_secret: String, ws_url: String) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let (command_tx, command_rx) = mpsc::channel(10);

        let manager = Self {
            ws_url: ws_url.clone(),
            api_key: api_key.clone(),
            api_secret: api_secret.clone(),
            event_tx: event_tx.clone(),
            subscribed_symbols: Arc::new(RwLock::new(Vec::new())),
            command_tx,
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
        };

        // Spawn background task
        manager.spawn_connection_task(command_rx);

        manager
    }

    /// Subscribe to market events (creates a new receiver from broadcast channel)
    pub fn subscribe(&self) -> broadcast::Receiver<MarketEvent> {
        self.event_tx.subscribe()
    }

    /// Update subscribed symbols dynamically without reconnecting
    pub async fn update_subscription(&self, symbols: Vec<String>) -> Result<()> {
        // Update our record first
        *self.subscribed_symbols.write().await = symbols.clone();

        // Send command to WebSocket task
        self.command_tx
            .send(SubscriptionCommand::UpdateSymbols(symbols))
            .await
            .map_err(|_| anyhow::anyhow!("Command channel closed"))?;
        Ok(())
    }

    /// Get current connection state
    pub async fn get_state(&self) -> ConnectionState {
        self.state.read().await.clone()
    }

    /// Spawn the background task that manages the persistent WebSocket connection
    fn spawn_connection_task(&self, mut command_rx: mpsc::Receiver<SubscriptionCommand>) {
        let ws_url = self.ws_url.clone();
        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();
        let event_tx = self.event_tx.clone();
        let state = self.state.clone();
        let subscribed_symbols = self.subscribed_symbols.clone();

        tokio::spawn(async move {
            let mut reconnect_attempts = 0u32;

            loop {
                if reconnect_attempts == 0 {
                    info!("WebSocketManager: Starting connection...");
                } else {
                    info!(
                        "WebSocketManager: Reconnection attempt #{}",
                        reconnect_attempts
                    );
                }

                match Self::run_connection(
                    &ws_url,
                    &api_key,
                    &api_secret,
                    &event_tx,
                    &state,
                    &subscribed_symbols,
                    &mut command_rx,
                )
                .await
                {
                    Ok(authenticated) => {
                        if authenticated {
                            info!("WebSocketManager: Connection ended cleanly after successful authentication");
                        } else {
                            info!("WebSocketManager: Connection ended before authentication");
                        }
                        break;
                    }
                    Err(e) => {
                        error!("WebSocketManager error: {}. Reconnecting...", e);
                        *state.write().await = ConnectionState::Disconnected;

                        // Exponential backoff: 0s, 1s, 2s, 4s, 8s, 16s, 30s (cap)
                        let delay_secs = if reconnect_attempts == 0 {
                            0
                        } else {
                            std::cmp::min(
                                2u64.pow(reconnect_attempts - 1),
                                MAX_RECONNECT_DELAY_SECS,
                            )
                        };

                        if delay_secs > 0 {
                            info!(
                                "WebSocketManager: Waiting {} seconds before reconnecting...",
                                delay_secs
                            );
                            time::sleep(Duration::from_secs(delay_secs)).await;
                        } else {
                            info!("WebSocketManager: Reconnecting immediately...");
                        }

                        reconnect_attempts += 1;
                    }
                }
            }
        });
    }

    /// Main connection loop
    /// Returns Ok(true) if connection was authenticated successfully, Ok(false) if ended before auth
    async fn run_connection(
        ws_url: &str,
        api_key: &str,
        api_secret: &str,
        event_tx: &broadcast::Sender<MarketEvent>,
        state: &Arc<RwLock<ConnectionState>>,
        subscribed_symbols: &Arc<RwLock<Vec<String>>>,
        command_rx: &mut mpsc::Receiver<SubscriptionCommand>,
    ) -> Result<bool> {
        // Connect to WebSocket
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to WebSocket")?;

        *state.write().await = ConnectionState::Connected;
        info!("WebSocketManager: Connected");

        let (mut write, mut read) = ws_stream.split();

        let mut authenticated = false;
        let mut current_subscribed: Vec<String> = Vec::new();

        // Heartbeat timers
        let mut ping_interval = time::interval(Duration::from_secs(PING_INTERVAL_SECS));
        ping_interval.tick().await; // First tick completes immediately
        let mut pong_deadline: Option<time::Instant> = None;

        loop {
            tokio::select! {
                // Read messages from WebSocket
                msg_result = read.next() => {
                    match msg_result {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(messages) = serde_json::from_str::<Vec<AlpacaMessage>>(&text) {
                                for message in messages {
                                    match message {
                                        AlpacaMessage::Welcome { msg } => {
                                            info!("WebSocketManager: Welcome - {}", msg);
                                        }
                                        AlpacaMessage::Success { msg } => {
                                            info!("WebSocketManager: Success - {}", msg);

                                            if msg == "connected" && !authenticated {
                                                // Send authentication
                                                let auth_msg = serde_json::json!({
                                                    "action": "auth",
                                                    "key": api_key,
                                                    "secret": api_secret
                                                });
                                                write.send(Message::Text(auth_msg.to_string().into())).await?;
                                                info!("WebSocketManager: Auth sent");
                                            } else if msg == "authenticated" {
                                                authenticated = true;
                                                *state.write().await = ConnectionState::Authenticated;
                                                info!("WebSocketManager: Authenticated");

                                                // Subscribe to initial symbols if any
                                                let initial = subscribed_symbols.read().await.clone();
                                                if !initial.is_empty() {
                                                    Self::send_subscription(&mut write, &initial).await?;
                                                    current_subscribed = initial.clone();
                                                    *state.write().await = ConnectionState::Subscribed;
                                                    info!("WebSocketManager: Restored subscription to {} symbols", initial.len());
                                                }
                                            }
                                        }
                                        AlpacaMessage::Error { code, msg } => {
                                            error!("WebSocketManager: Alpaca error ({}): {}", code, msg);
                                        }
                                        AlpacaMessage::Subscription { trades, quotes } => {
                                            info!("WebSocketManager: Subscribed - Trades: {:?}, Quotes: {:?}", trades, quotes);
                                        }
                                        AlpacaMessage::Quote(quote) => {
                                            let mid_price = (quote.bid_price + quote.ask_price) / 2.0;
                                            let event = MarketEvent::Quote {
                                                symbol: quote.symbol,
                                                price: Decimal::from_f64_retain(mid_price).unwrap_or(Decimal::ZERO),
                                                timestamp: Utc::now().timestamp_millis(),
                                            };
                                            let _ = event_tx.send(event);
                                        }
                                        AlpacaMessage::Trade(trade) => {
                                            let event = MarketEvent::Quote {
                                                symbol: trade.symbol,
                                                price: Decimal::from_f64_retain(trade.price).unwrap_or(Decimal::ZERO),
                                                timestamp: Utc::now().timestamp_millis(),
                                            };
                                            let _ = event_tx.send(event);
                                        }
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            // Received pong response
                            if pong_deadline.is_some() {
                                pong_deadline = None;
                                debug!("WebSocketManager: Pong received");
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("WebSocketManager: Connection closed by server");
                            return Ok(authenticated);
                        }
                        Some(Err(e)) => {
                            error!("WebSocketManager: WebSocket error: {}", e);
                            return Err(e.into());
                        }
                        None => {
                            warn!("WebSocketManager: Stream ended");
                            return Ok(authenticated);
                        }
                        _ => {}
                    }
                }

                // Send periodic pings (heartbeat)
                _ = ping_interval.tick() => {
                    if authenticated {
                        write.send(Message::Ping(vec![].into())).await?;
                        pong_deadline = Some(time::Instant::now() + Duration::from_secs(PONG_TIMEOUT_SECS));
                        debug!("WebSocketManager: Ping sent, waiting for pong...");
                    }
                }

                // Check for pong timeout
                _ = async {
                    if let Some(deadline) = pong_deadline {
                        time::sleep_until(deadline).await;
                    } else {
                        // If no deadline, wait forever (this branch won't be selected)
                        std::future::pending::<()>().await;
                    }
                }, if pong_deadline.is_some() => {
                    error!("WebSocketManager: Pong timeout - connection appears dead");
                    return Err(anyhow::anyhow!("Pong timeout"));
                }

                // Handle subscription update commands
                Some(cmd) = command_rx.recv() => {
                    match cmd {
                        SubscriptionCommand::UpdateSymbols(new_symbols) => {
                            if authenticated && new_symbols != current_subscribed {
                                info!("WebSocketManager: Updating subscription to: {:?}", new_symbols);
                                Self::send_subscription(&mut write, &new_symbols).await?;
                                current_subscribed = new_symbols;
                                *state.write().await = ConnectionState::Subscribed;
                            } else if !authenticated {
                                warn!("WebSocketManager: Cannot update subscription - not authenticated yet");
                            }
                        }
                        SubscriptionCommand::Shutdown => {
                            info!("WebSocketManager: Shutdown command received");
                            return Ok(authenticated);
                        }
                    }
                }
            }
        }
    }

    /// Send subscription message via WebSocket
    async fn send_subscription(
        write: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        symbols: &[String],
    ) -> Result<()> {
        let subscribe_msg = serde_json::json!({
            "action": "subscribe",
            "quotes": symbols,
            "trades": symbols
        });
        write
            .send(Message::Text(subscribe_msg.to_string().into()))
            .await
            .context("Failed to send subscription message")?;
        info!(
            "WebSocketManager: Subscription message sent for: {:?}",
            symbols
        );
        Ok(())
    }
}

impl Drop for AlpacaWebSocketManager {
    fn drop(&mut self) {
        // Send shutdown command (best effort)
        let _ = self.command_tx.try_send(SubscriptionCommand::Shutdown);
    }
}
