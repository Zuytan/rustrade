use crate::infrastructure::observability::Metrics;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tokio::sync::RwLock;

/// Health status of an agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Dead,
    // "Dead" might be inferred from timeout, but explicit state is useful
    Starting,
}

impl HealthStatus {
    pub fn to_metric_value(&self) -> f64 {
        match self {
            HealthStatus::Healthy => 1.0,
            HealthStatus::Degraded => 0.5,
            HealthStatus::Dead => 0.0,
            HealthStatus::Starting => 0.1,
        }
    }
}

/// Status of a specific agent
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentStatus {
    pub name: String,
    pub health: HealthStatus,
    pub last_heartbeat: DateTime<Utc>,
    pub message: Option<String>,
    pub metrics: HashMap<String, String>, // Key-Value pairs for specific metrics
}

/// Registry to track the status of all agents in the system
pub struct AgentStatusRegistry {
    statuses: RwLock<HashMap<String, AgentStatus>>,
    metrics: Metrics,
}

impl AgentStatusRegistry {
    pub fn new(metrics: Metrics) -> Self {
        Self {
            statuses: RwLock::new(HashMap::new()),
            metrics,
        }
    }

    /// Update the heartbeat of an agent
    pub async fn update_heartbeat(&self, name: &str, health: HealthStatus) {
        let mut statuses = self.statuses.write().await;

        // Update Prometheus
        self.metrics
            .agent_up
            .with_label_values(&[name])
            .set(health.to_metric_value());
        self.metrics
            .agent_last_heartbeat
            .with_label_values(&[name])
            .set(Utc::now().timestamp() as f64);

        if let Some(status) = statuses.get_mut(name) {
            status.health = health;
            status.last_heartbeat = Utc::now();
        } else {
            statuses.insert(
                name.to_string(),
                AgentStatus {
                    name: name.to_string(),
                    health,
                    last_heartbeat: Utc::now(),
                    message: None,
                    metrics: HashMap::new(),
                },
            );
        }
    }

    /// Update a specific metric for an agent
    pub async fn update_metric(&self, name: &str, key: &str, value: String) {
        let mut statuses = self.statuses.write().await;
        if let Some(status) = statuses.get_mut(name) {
            status.metrics.insert(key.to_string(), value);
            status.last_heartbeat = Utc::now(); // Updating metric counts as ALIVE
        }
        // If agent doesn't exist yet, we might want to create it or wait for heartbeat.
        // For now, assume heartbeat comes first or we ignore.
    }

    /// Get all agent statuses
    pub async fn get_all(&self) -> HashMap<String, AgentStatus> {
        self.statuses.read().await.clone()
    }

    /// Get status for a specific agent
    pub async fn get_status(&self, name: &str) -> Option<AgentStatus> {
        self.statuses.read().await.get(name).cloned()
    }

    /// Get all agent statuses synchronously (non-blocking, returns empty if lock is held)
    pub fn get_all_sync(&self) -> HashMap<String, AgentStatus> {
        if let Ok(guard) = self.statuses.try_read() {
            guard.clone()
        } else {
            // If lock is held by a writer, we skip this frame update
            HashMap::new()
        }
    }
}
