use crate::domain::ports::NotificationService;
use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};

/// A simple notification service that logs warnings to the console/system logger.
/// Useful as a fallback, for local dry runs, or as a base service.
pub struct LoggingNotificationService;

#[async_trait]
impl NotificationService for LoggingNotificationService {
    async fn send_notification(&self, title: &str, message: &str) -> Result<()> {
        warn!(
            "🔔 NOTIFICATION [{}] - TITLE: {} | MESSAGE: {}",
            chrono::Utc::now().to_rfc3339(),
            title,
            message
        );
        Ok(())
    }
}

/// A multi-channel notification service that can route notifications to various channels
/// like Slack, PagerDuty, Discord, etc.
pub struct MultiChannelNotificationService {
    slack_webhook_url: Option<String>,
    pagerduty_integration_key: Option<String>,
}

impl MultiChannelNotificationService {
    /// Create a new MultiChannelNotificationService from environment variables or direct parameters
    pub fn new(
        slack_webhook_url: Option<String>,
        pagerduty_integration_key: Option<String>,
    ) -> Self {
        Self {
            slack_webhook_url,
            pagerduty_integration_key,
        }
    }

    /// Helper to build the service from env parameters
    pub fn from_env() -> Self {
        let slack = std::env::var("SLACK_WEBHOOK_URL").ok();
        let pagerduty = std::env::var("PAGERDUTY_INTEGRATION_KEY").ok();
        Self::new(slack, pagerduty)
    }
}

#[async_trait]
impl NotificationService for MultiChannelNotificationService {
    async fn send_notification(&self, title: &str, message: &str) -> Result<()> {
        // 1. Always log to system warn logs
        warn!("🔔 ALERT: {} - {}", title, message);

        // 2. If Slack is configured, send HTTP request (mocked for now, ready for reqwest integrations)
        if let Some(ref url) = self.slack_webhook_url {
            info!("Slack Notification sent to {} (Mocked/Log Only)", url);
        }

        // 3. If PagerDuty is configured, trigger alert (mocked for now)
        if let Some(ref key) = self.pagerduty_integration_key {
            info!(
                "PagerDuty Trigger Alert sent using key {} (Mocked/Log Only)",
                key
            );
        }

        Ok(())
    }
}
