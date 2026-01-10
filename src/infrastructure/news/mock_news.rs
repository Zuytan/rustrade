use crate::domain::listener::NewsEvent;
use crate::domain::ports::NewsDataService;
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc::{self, Receiver};
use tracing::info;
use uuid::Uuid;

pub struct MockNewsService;

impl MockNewsService {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockNewsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NewsDataService for MockNewsService {
    async fn subscribe_news(&self) -> Result<Receiver<NewsEvent>> {
        let (tx, rx) = mpsc::channel(100);

        // Spawn a task to generate mock news
        tokio::spawn(async move {
            info!("Starting Mock News Generator...");

            // Wait a bit before sending the first news
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            // Scenario 1: Elon Musk Tweet (Buy Signal)
            let event1 = NewsEvent {
                id: Uuid::new_v4().to_string(),
                source: "Twitter".to_string(),
                title: "Elon Musk Tweet".to_string(),
                content: "Dogecoin is the future currency of Earth".to_string(),
                url: Some("https://twitter.com/elonmusk/status/123456789".to_string()),
                timestamp: Utc::now(),
                sentiment_score: Some(0.9),
            };

            if let Err(e) = tx.send(event1).await {
                info!("Receiver dropped, stopping mock news generator: {}", e);
                return;
            }
            info!("Sent mock news: Elon Musk Tweet");

            // Wait...
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

            // Scenario 2: SEC Lawsuit (Sell Signal)
            let event2 = NewsEvent {
                id: Uuid::new_v4().to_string(),
                source: "CryptoPanic".to_string(),
                title: "SEC Lawsuit".to_string(),
                content: "SEC files lawsuit against Binance for securities violations".to_string(),
                url: Some("https://cryptopanic.com/news/123".to_string()),
                timestamp: Utc::now(),
                sentiment_score: Some(-0.9),
            };

            if let Err(e) = tx.send(event2).await {
                info!("Receiver dropped, stopping mock news generator: {}", e);
                return;
            }
            info!("Sent mock news: SEC Lawsuit");
        });

        Ok(rx)
    }
}
