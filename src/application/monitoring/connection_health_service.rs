use chrono::{DateTime, Utc};
use tokio::sync::{RwLock, broadcast};
use tracing::info;

/// Status of a specific connection component
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ConnectionStatus {
    Online,
    Degraded,
    Offline,
}

/// Event broadcast when connection status changes
#[derive(Debug, Clone)]
pub struct ConnectionHealthEvent {
    pub component: String,
    pub status: ConnectionStatus,
    pub timestamp: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Centralized service to monitor and broadcast connectivity status across the system
pub struct ConnectionHealthService {
    market_data: RwLock<ConnectionStatus>,
    execution: RwLock<ConnectionStatus>,
    event_tx: broadcast::Sender<ConnectionHealthEvent>,
}

impl ConnectionHealthService {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            market_data: RwLock::new(ConnectionStatus::Offline),
            execution: RwLock::new(ConnectionStatus::Offline),
            event_tx,
        }
    }

    /// Update status for market data connection
    pub async fn set_market_data_status(&self, status: ConnectionStatus, reason: Option<String>) {
        let mut lock = self.market_data.write().await;
        if *lock != status {
            *lock = status;
            self.broadcast_change("MarketData", status, reason).await;
        }
    }

    /// Update status for execution connection
    pub async fn set_execution_status(&self, status: ConnectionStatus, reason: Option<String>) {
        let mut lock = self.execution.write().await;
        if *lock != status {
            *lock = status;
            self.broadcast_change("Execution", status, reason).await;
        }
    }

    /// Subscribe to connection health events
    pub fn subscribe(&self) -> broadcast::Receiver<ConnectionHealthEvent> {
        self.event_tx.subscribe()
    }

    /// Get current market data status
    pub async fn get_market_data_status(&self) -> ConnectionStatus {
        *self.market_data.read().await
    }

    /// Get current execution status
    pub async fn get_execution_status(&self) -> ConnectionStatus {
        *self.execution.read().await
    }

    /// Internal helper to broadcast change
    async fn broadcast_change(
        &self,
        component: &str,
        status: ConnectionStatus,
        reason: Option<String>,
    ) {
        let event = ConnectionHealthEvent {
            component: component.to_string(),
            status,
            timestamp: Utc::now(),
            reason,
        };

        info!(
            "ConnectionHealthService: {} is now {:?}{}",
            component,
            status,
            event
                .reason
                .as_ref()
                .map(|r| format!(" ({})", r))
                .unwrap_or_default()
        );

        let _ = self.event_tx.send(event);
    }
}

impl Default for ConnectionHealthService {
    fn default() -> Self {
        Self::new()
    }
}
