use crate::application::monitoring::connection_health_service::{
    ConnectionHealthService, ConnectionStatus,
};
use crate::config::ApiServerConfig;
use crate::domain::ports::ExecutionService;
use crate::domain::trading::portfolio::Portfolio;
use crate::infrastructure::observability::Metrics;
use axum::{
    Json, Router,
    extract::{
        Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
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
    pub metrics: Metrics,
    pub connection_health: Arc<ConnectionHealthService>,
    pub started_at: std::time::Instant,
}

impl ApiState {
    pub fn new(
        portfolio: Arc<RwLock<Portfolio>>,
        execution_service: Arc<dyn ExecutionService>,
        log_sender: broadcast::Sender<String>,
        metrics: Metrics,
        connection_health: Arc<ConnectionHealthService>,
    ) -> Self {
        Self {
            portfolio,
            execution_service,
            log_sender,
            metrics,
            connection_health,
            started_at: std::time::Instant::now(),
        }
    }
}

/// Header carrying the API key. Query-string keys are deliberately not supported,
/// as they leak into proxy and access logs.
pub const API_KEY_HEADER: &str = "X-API-Key";

/// Run the Axum REST, WebSocket and Prometheus `/metrics` server.
pub async fn run_api_server(config: &ApiServerConfig, state: ApiState) -> anyhow::Result<()> {
    let app = build_router(state, config.api_key.as_deref().map(Arc::from));

    let addr = config.socket_addr();
    tracing::info!(
        "API Server listening on http://{} (auth: {})",
        addr,
        if config.api_key.is_some() {
            "X-API-Key required"
        } else {
            "none, loopback only"
        }
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Build the API router. When `api_key` is set, every route requires it.
fn build_router(state: ApiState, api_key: Option<Arc<str>>) -> Router {
    Router::new()
        .route("/api/v1/portfolio", get(get_portfolio))
        .route("/api/v1/positions", get(get_positions))
        .route("/api/v1/ws", get(ws_handler))
        .route("/metrics", get(get_metrics))
        .route_layer(middleware::from_fn_with_state(api_key, require_api_key))
        .with_state(state)
}

async fn require_api_key(
    State(expected): State<Option<Arc<str>>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = expected else {
        return Ok(next.run(request).await);
    };

    let provided = request
        .headers()
        .get(API_KEY_HEADER)
        .map(|v| v.as_bytes())
        .unwrap_or_default();

    if constant_time_eq(provided, expected.as_bytes()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Compare secrets without short-circuiting on the first differing byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn get_portfolio(State(state): State<ApiState>) -> impl IntoResponse {
    let p = state.portfolio.read().await;
    Json(p.clone())
}

async fn get_positions(State(state): State<ApiState>) -> impl IntoResponse {
    let p = state.portfolio.read().await;
    Json(p.positions.clone())
}

/// Prometheus scrape endpoint. Gauges derived from shared state are refreshed at
/// scrape time, so they are current even when the stdout reporter is disabled.
async fn get_metrics(State(state): State<ApiState>) -> impl IntoResponse {
    for (component, status) in [
        (
            "market_data",
            state.connection_health.get_market_data_status().await,
        ),
        (
            "execution",
            state.connection_health.get_execution_status().await,
        ),
    ] {
        let up = if status == ConnectionStatus::Online {
            1.0
        } else {
            0.0
        };
        state
            .metrics
            .connection_up
            .with_label_values(&[component])
            .set(up);
    }

    {
        let p = state.portfolio.read().await;
        if let Some(cash) = rust_decimal::prelude::ToPrimitive::to_f64(&p.cash) {
            state.metrics.portfolio_cash_usd.set(cash);
        }
        state.metrics.positions_count.set(p.positions.len() as f64);
    }
    state
        .metrics
        .uptime_seconds
        .set(state.started_at.elapsed().as_secs_f64());

    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::mock::MockExecutionService;

    const KEY: &str = "0123456789abcdef0123";

    /// Serve the router on an ephemeral loopback port and return its base URL.
    async fn spawn_server(api_key: Option<&str>) -> String {
        let portfolio = Arc::new(RwLock::new(Portfolio::new()));
        let health = Arc::new(ConnectionHealthService::new());
        health
            .set_execution_status(ConnectionStatus::Online, None)
            .await;
        let state = ApiState::new(
            portfolio.clone(),
            Arc::new(MockExecutionService::new(portfolio)),
            broadcast::channel(16).0,
            Metrics::new().expect("metrics"),
            health,
        );
        let app = build_router(state, api_key.map(Arc::from));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    async fn status(url: &str, key: Option<&str>) -> u16 {
        let mut req = reqwest::Client::new().get(url);
        if let Some(k) = key {
            req = req.header(API_KEY_HEADER, k);
        }
        req.send().await.expect("request").status().as_u16()
    }

    #[tokio::test]
    async fn test_protected_routes_reject_missing_or_wrong_key() {
        let base = spawn_server(Some(KEY)).await;
        for path in [
            "/api/v1/portfolio",
            "/api/v1/positions",
            "/api/v1/ws",
            "/metrics",
        ] {
            let url = format!("{base}{path}");
            assert_eq!(status(&url, None).await, 401, "{path} without key");
            assert_eq!(
                status(&url, Some("wrong-key")).await,
                401,
                "{path} wrong key"
            );
        }
    }

    #[tokio::test]
    async fn test_valid_key_is_accepted() {
        let base = spawn_server(Some(KEY)).await;
        let url = format!("{base}/api/v1/portfolio");
        assert_eq!(status(&url, Some(KEY)).await, 200);
    }

    #[tokio::test]
    async fn test_key_in_query_string_is_not_accepted() {
        // Query strings end up in proxy/access logs: only the header is honored.
        let base = spawn_server(Some(KEY)).await;
        let url = format!("{base}/api/v1/portfolio?api_key={KEY}");
        assert_eq!(status(&url, None).await, 401);
    }

    #[tokio::test]
    async fn test_no_key_configured_allows_access() {
        // Only reachable on loopback: ApiServerConfig refuses public binds without a key.
        let base = spawn_server(None).await;
        let url = format!("{base}/api/v1/positions");
        assert_eq!(status(&url, None).await, 200);
    }

    #[tokio::test]
    async fn test_metrics_endpoint_exports_prometheus_text() {
        let base = spawn_server(Some(KEY)).await;
        let resp = reqwest::Client::new()
            .get(format!("{base}/metrics"))
            .header(API_KEY_HEADER, KEY)
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status().as_u16(), 200);
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(content_type.starts_with("text/plain"));
        let body = resp.text().await.expect("body");
        assert!(body.contains("rustrade_connection_up{component=\"execution\"} 1"));
        assert!(body.contains("rustrade_connection_up{component=\"market_data\"} 0"));
        assert!(body.contains("rustrade_uptime_seconds"));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
    }
}
