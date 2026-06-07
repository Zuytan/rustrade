use crate::domain::ports::ExecutionService;
use crate::domain::trading::portfolio::Portfolio;
use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::field::Visit;
use tracing_subscriber::Layer;

/// Custom tracing layer to broadcast formatted log messages to WebSocket clients.
pub struct BroadcastLogLayer {
    sender: broadcast::Sender<String>,
}

impl BroadcastLogLayer {
    pub fn new(sender: broadcast::Sender<String>) -> Self {
        Self { sender }
    }
}

struct StringVisitor {
    message: String,
}

impl Visit for StringVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}

impl<S> Layer<S> for BroadcastLogLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = StringVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);
        if !visitor.message.is_empty() {
            let _ = self.sender.send(visitor.message);
        }
    }
}

#[derive(Clone)]
pub struct ApiState {
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub execution_service: Arc<dyn ExecutionService>,
    pub log_sender: broadcast::Sender<String>,
}

/// Run the Axum REST and WebSocket server.
pub async fn run_api_server(
    port: u16,
    portfolio: Arc<RwLock<Portfolio>>,
    execution_service: Arc<dyn ExecutionService>,
    log_sender: broadcast::Sender<String>,
) -> anyhow::Result<()> {
    let state = ApiState {
        portfolio,
        execution_service,
        log_sender,
    };

    let app = Router::new()
        .route("/api/v1/portfolio", get(get_portfolio))
        .route("/api/v1/positions", get(get_positions))
        .route("/api/v1/ws", get(ws_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("API Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn get_portfolio(State(state): State<ApiState>) -> impl IntoResponse {
    let p = state.portfolio.read().await;
    Json(p.clone())
}

async fn get_positions(State(state): State<ApiState>) -> impl IntoResponse {
    let p = state.portfolio.read().await;
    Json(p.positions.clone())
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<ApiState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: ApiState) {
    let mut log_rx = state.log_sender.subscribe();
    let mut order_rx = match state.execution_service.subscribe_order_updates().await {
        Ok(rx) => rx,
        Err(e) => {
            tracing::error!("WS: Failed to subscribe to order updates: {}", e);
            return;
        }
    };

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));

    loop {
        tokio::select! {
            // 1. Logs
            Ok(log_msg) = log_rx.recv() => {
                let payload = serde_json::json!({
                    "type": "log",
                    "data": log_msg
                });
                if let Ok(text) = serde_json::to_string(&payload) {
                    let res = socket.send(Message::Text(text.into())).await;
                    if res.is_err() {
                        break;
                    }
                }
            }
            // 2. Order Updates
            Ok(order_update) = order_rx.recv() => {
                let payload = serde_json::json!({
                    "type": "order_update",
                    "data": order_update
                });
                if let Ok(text) = serde_json::to_string(&payload) {
                    let res = socket.send(Message::Text(text.into())).await;
                    if res.is_err() {
                        break;
                    }
                }
            }
            // 3. Periodic PnL / Equity Update
            _ = interval.tick() => {
                let p = state.portfolio.read().await;
                let payload = serde_json::json!({
                    "type": "portfolio",
                    "data": {
                        "cash": p.cash,
                        "positions_count": p.positions.len(),
                        "synchronized": p.synchronized
                    }
                });
                if let Ok(text) = serde_json::to_string(&payload) {
                    let res = socket.send(Message::Text(text.into())).await;
                    if res.is_err() {
                        break;
                    }
                }
            }
        }
    }
}
