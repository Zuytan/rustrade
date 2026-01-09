use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::domain::listener::{ListenerConfig, ListenerRule, NewsEvent, ListenerAction};
use crate::domain::ports::NewsDataService;

pub struct ListenerAgent {
    news_service: Arc<dyn NewsDataService>,
    config: ListenerConfig,
    analyst_cmd_tx: mpsc::Sender<crate::application::agents::analyst::AnalystCommand>,
}

impl ListenerAgent {
    pub fn new(
        news_service: Arc<dyn NewsDataService>,
        config: ListenerConfig,
        analyst_cmd_tx: mpsc::Sender<crate::application::agents::analyst::AnalystCommand>,
    ) -> Self {
        Self {
            news_service,
            config,
            analyst_cmd_tx,
        }
    }

    pub async fn run(&self) {
        info!("Listener Agent started.");

        let mut news_rx = match self.news_service.subscribe_news().await {
            Ok(rx) => rx,
            Err(e) => {
                error!("Failed to subscribe to news service: {}. Listener Agent stopping.", e);
                return;
            }
        };

        while let Some(event) = news_rx.recv().await {
            info!("Received news event: {} - {}", event.source, event.title);
            self.process_event(&event).await;
        }

        warn!("Listener Agent stopped (stream ended).");
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

        info!("Listener: Sending News Signal to Analyst: {:?} for {}", signal.sentiment, signal.symbol);

        self.analyst_cmd_tx.send(crate::application::agents::analyst::AnalystCommand::ProcessNews(signal))
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
            Ok(self.rx.lock().await.take().unwrap())
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
                action: ListenerAction::NotifyAnalyst(crate::domain::listener::NewsSentiment::Bullish),
                active: true,
            }],
        };

        let agent = ListenerAgent::new(news_service, config, analyst_cmd_tx);

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
        news_tx.send(event).await.unwrap();

        // Check command
        let cmd = analyst_cmd_rx.recv().await;
        assert!(cmd.is_some());
        match cmd.unwrap() {
            crate::application::agents::analyst::AnalystCommand::ProcessNews(signal) => {
                 assert_eq!(signal.symbol, "TEST/USD");
                 assert_eq!(signal.sentiment, crate::domain::listener::NewsSentiment::Bullish);
            }
            _ => panic!("Expected ProcessNews command"),
        }
    }
}
