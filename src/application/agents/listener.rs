use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};

use crate::domain::listener::{ListenerAction, ListenerConfig, ListenerRule, NewsEvent};
use crate::domain::ports::NewsDataService;

pub struct ListenerAgent {
    news_service: Arc<dyn NewsDataService>,
    config: ListenerConfig,
    analyst_cmd_tx: mpsc::Sender<crate::application::agents::analyst::AnalystCommand>,
    /// Optional broadcast sender to forward news events to UI
    news_broadcast_tx: Option<broadcast::Sender<NewsEvent>>,
    agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
}

impl ListenerAgent {
    pub fn new(
        news_service: Arc<dyn NewsDataService>,
        config: ListenerConfig,
        analyst_cmd_tx: mpsc::Sender<crate::application::agents::analyst::AnalystCommand>,
        agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    ) -> Self {
        Self {
            news_service,
            config,
            analyst_cmd_tx,
            news_broadcast_tx: None,
            agent_registry,
        }
    }

    /// Create a ListenerAgent with a news broadcast sender for UI updates
    pub fn with_news_broadcast(
        news_service: Arc<dyn NewsDataService>,
        config: ListenerConfig,
        analyst_cmd_tx: mpsc::Sender<crate::application::agents::analyst::AnalystCommand>,
        news_broadcast_tx: broadcast::Sender<NewsEvent>,
        agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    ) -> Self {
        Self {
            news_service,
            config,
            analyst_cmd_tx,
            news_broadcast_tx: Some(news_broadcast_tx),
            agent_registry,
        }
    }

    pub async fn run(&self) {
        info!("Listener Agent started.");

        let mut reconnect_attempts = 0;
        const MAX_BACKOFF_MS: u64 = 60000;

        // Initial Heartbeat
        self.agent_registry
            .update_heartbeat(
                "Listener",
                crate::application::monitoring::agent_status::HealthStatus::Healthy,
            )
            .await;

        loop {
            // Heartbeat check (simple implementation inside the loop)
            // Note: This loop blocks on news_rx.recv() below, so this heartbeat only updates
            // when a connection cycle starts/restarts or an event is received if we were to put it after recv.
            // Ideally, the ListenerAgent should have a select! with a ticker like other agents.
            // Refactoring to use select! for consistent heartbeats.
            self.agent_registry
                .update_heartbeat(
                    "Listener",
                    if reconnect_attempts > 0 {
                        crate::application::monitoring::agent_status::HealthStatus::Degraded
                    } else {
                        crate::application::monitoring::agent_status::HealthStatus::Healthy
                    },
                )
                .await;

            reconnect_attempts += 1;
            let backoff_ms =
                (1000 * (2_u64.pow(reconnect_attempts.min(6) - 1))).min(MAX_BACKOFF_MS);

            if reconnect_attempts > 1 {
                warn!("Listener Agent: Reconnecting in {}ms...", backoff_ms);
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            }

            // Mark as Starting/Degraded during reconnection
            self.agent_registry
                .update_heartbeat(
                    "Listener",
                    crate::application::monitoring::agent_status::HealthStatus::Degraded,
                )
                .await;

            let mut news_rx = match self.news_service.subscribe_news().await {
                Ok(rx) => {
                    info!("Successfully subscribed to news service.");
                    reconnect_attempts = 0; // Reset on success
                    self.agent_registry
                        .update_heartbeat(
                            "Listener",
                            crate::application::monitoring::agent_status::HealthStatus::Healthy,
                        )
                        .await;
                    rx
                }
                Err(e) => {
                    error!("Failed to subscribe to news service: {}. Retrying...", e);
                    continue;
                }
            };

            let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(5));

            loop {
                tokio::select! {
                     _ = heartbeat_interval.tick() => {
                        self.agent_registry.update_heartbeat(
                            "Listener",
                            crate::application::monitoring::agent_status::HealthStatus::Healthy
                        ).await;
                     }

                     maybe_event = news_rx.recv() => {
                         match maybe_event {
                             Some(event) => {
                                info!("Received news event: {} - {}", event.source, event.title);

                                // Forward to UI broadcast if available
                                if let Some(tx) = &self.news_broadcast_tx {
                                    let _ = tx.send(event.clone());
                                }

                                self.process_event(&event).await;
                             }
                             None => {
                                 warn!("Listener Agent news stream ended. Re-establishing...");
                                 break; // Break inner loop to reconnect
                             }
                         }
                     }
                }
            }
        }
    }

    async fn process_event(&self, event: &NewsEvent) {
        for rule in &self.config.rules {
            if !rule.active {
                continue;
            }

            if rule.matches(&event.content) || rule.matches(&event.title) {
                info!("News matched rule '{}': {}", rule.id, event.title);
                if let Err(e) = self.trigger_action(rule, event).await {
                    error!("Failed to trigger action for rule '{}': {}", rule.id, e);
                }
            }
        }
    }

    async fn trigger_action(&self, rule: &ListenerRule, event: &NewsEvent) -> Result<()> {
        let sentiment = match rule.action {
            ListenerAction::NotifyAnalyst(s) => s,
            // Map legacy actions to sentiment
            ListenerAction::BuyImmediate => crate::domain::listener::NewsSentiment::Bullish,
            ListenerAction::SellImmediate => crate::domain::listener::NewsSentiment::Bearish,
        };

        let signal = crate::domain::listener::NewsSignal {
            symbol: rule.target_symbol.clone(),
            sentiment,
            headline: event.title.clone(),
            source: event.source.clone(),
            url: event.url.clone(),
        };

        info!(
            "Listener: Sending News Signal to Analyst: {:?} for {}",
            signal.sentiment, signal.symbol
        );

        self.analyst_cmd_tx
            .send(crate::application::agents::analyst::AnalystCommand::ProcessNews(signal))
            .await
            .context("Failed to send news signal to analyst")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    // Mock News Service for Unit Test (Simpler than the infra one)
    struct TestNewsService {
        rx: tokio::sync::Mutex<Option<mpsc::Receiver<NewsEvent>>>,
    }

    #[async_trait::async_trait]
    impl NewsDataService for TestNewsService {
        async fn subscribe_news(&self) -> Result<mpsc::Receiver<NewsEvent>> {
            Ok(self
                .rx
                .lock()
                .await
                .take()
                .expect("Command receiver must be initialized at startup"))
        }
    }

    #[tokio::test]
    async fn test_listener_agent_matches_and_proposes() {
        let (news_tx, news_rx) = mpsc::channel(10);
        let (analyst_cmd_tx, mut analyst_cmd_rx) = mpsc::channel(10);

        let news_service = Arc::new(TestNewsService {
            rx: tokio::sync::Mutex::new(Some(news_rx)),
        });

        let config = ListenerConfig {
            poll_interval_seconds: 1,
            rules: vec![ListenerRule {
                id: "test-rule".to_string(),
                keywords: vec!["Buy".to_string(), "Now".to_string()],
                target_symbol: "TEST/USD".to_string(),
                action: ListenerAction::NotifyAnalyst(
                    crate::domain::listener::NewsSentiment::Bullish,
                ),
                active: true,
            }],
        };

        let agent_registry = Arc::new(
            crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                crate::infrastructure::observability::Metrics::new().unwrap(),
            ),
        );
        let agent = ListenerAgent::new(news_service, config, analyst_cmd_tx, agent_registry);

        // Spawn agent
        tokio::spawn(async move {
            agent.run().await;
        });

        // Send matching news
        let event = NewsEvent {
            id: Uuid::new_v4().to_string(),
            source: "Test".to_string(),
            title: "Signal".to_string(),
            content: "You should Buy Now!".to_string(),
            url: None,
            timestamp: Utc::now(),
            sentiment_score: None,
        };
        if let Err(e) = news_tx.send(event).await {
            tracing::error!("Failed to forward news event to channel: {}", e);
        }

        // Check command
        let cmd = analyst_cmd_rx.recv().await;
        assert!(cmd.is_some());
        match cmd.expect("Command must be present in test channel") {
            crate::application::agents::analyst::AnalystCommand::ProcessNews(signal) => {
                assert_eq!(signal.symbol, "TEST/USD");
                assert_eq!(
                    signal.sentiment,
                    crate::domain::listener::NewsSentiment::Bullish
                );
            }
            _ => panic!("Expected ProcessNews command"),
        }
    }
}
