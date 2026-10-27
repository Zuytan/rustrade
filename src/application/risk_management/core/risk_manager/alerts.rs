//! Alert delivery: webhook and notification service for circuit breaker events.

use super::*;

impl RiskManager {
    /// Set webhook URL for alerts
    pub fn set_alert_webhook_url(&mut self, url: Option<String>) {
        self.alert_webhook_url = url;
    }

    /// Set notification service for emergency alerts
    pub fn set_notification_service(&mut self, service: Arc<dyn NotificationService>) {
        self.liquidation_service.set_notification_service(service);
    }

    /// Helper to emit structured tracing event and send asynchronous webhook alert
    pub(super) fn trigger_alert(&self, level: HaltLevel, reason: &str) {
        // Structured tracing log with dedicated event name
        tracing::error!(
            event = "circuit_breaker_triggered",
            level = ?level,
            reason = %reason,
            "CIRCUIT BREAKER TRIGGERED ({:?}): {}",
            level,
            reason
        );

        if let Some(ref webhook_url) = self.alert_webhook_url {
            let webhook_url = webhook_url.clone();
            let reason = reason.to_string();
            let level_str = format!("{:?}", level);
            let timestamp = Utc::now().to_rfc3339();

            tokio::spawn(async move {
                let payload = serde_json::json!({
                    "event": "circuit_breaker_triggered",
                    "level": level_str,
                    "reason": reason,
                    "timestamp": timestamp,
                    "message": format!("⚠️ [CIRCUIT BREAKER] Level: {}, Reason: {}", level_str, reason)
                });

                let client = reqwest::Client::new();
                match client.post(&webhook_url).json(&payload).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::warn!(
                                "Failed to send alert to webhook. Status: {}",
                                resp.status()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send alert to webhook: {}", e);
                    }
                }
            });
        }
    }
}
