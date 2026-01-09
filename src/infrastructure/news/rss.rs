use crate::domain::listener::NewsEvent;
use crate::domain::ports::NewsDataService;
use crate::infrastructure::news::sentiment_analyzer::SentimentAnalyzer;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use rss::Channel;
use std::collections::HashSet;
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver};
use tokio::sync::Mutex;
use tracing::{error, info, debug};
use uuid::Uuid;

pub struct RssNewsService {
    url: String,
    client: Client,
    seen_guids: Arc<Mutex<HashSet<String>>>,
    poll_interval_seconds: u64,
    sentiment_analyzer: Arc<SentimentAnalyzer>,
}

impl RssNewsService {
    pub fn new(url: &str, poll_interval_seconds: u64) -> Self {
        Self {
            url: url.to_string(),
            client: Client::new(),
            seen_guids: Arc::new(Mutex::new(HashSet::new())),
            poll_interval_seconds,
            sentiment_analyzer: Arc::new(SentimentAnalyzer::new()),
        }
    }


}

#[async_trait]
impl NewsDataService for RssNewsService {
    async fn subscribe_news(&self) -> Result<Receiver<NewsEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let url = self.url.clone();
        let client = self.client.clone();
        let seen_guids = self.seen_guids.clone();
        let interval_sec = self.poll_interval_seconds;
        let sentiment_analyzer = self.sentiment_analyzer.clone();

        tokio::spawn(async move {
            info!("Starting RSS News Poller for: {} (with NLP sentiment analysis)", url);
            
            // Initial fetch to populate seen_guids without sending events (optional, or we can send them)
            // For now, let's treat the first fetch as "historical" and not trigger actions, 
            // OR let's trigger actions for very recent items.
            // A common pattern is to fetch once to fill the cache, then fetch loop.
            
            // Let's populate seen_guids first to avoid flooding on restart
            // Let's populate seen_guids first to avoid flooding on restart
            let fetch_result = async {
                let content = client.get(&url).send().await?;
                let bytes = content.bytes().await?;
                match Channel::read_from(Cursor::new(bytes)) {
                    Ok(c) => Ok(c),
                    Err(e) => Err(anyhow::anyhow!(e)),
                }
            }.await;

            if let Ok(channel) = fetch_result {
                let mut guids = seen_guids.lock().await;
                for item in channel.items() {
                    if let Some(guid) = item.guid() {
                        guids.insert(guid.value.to_string());
                    }
                }
                info!("Initialized RSS Poller: Marked {} items as seen.", guids.len());
            }

            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(interval_sec)).await;

                debug!("Polling RSS feed...");
                let content_result = client.get(&url).send().await;
                
                match content_result {
                    Ok(resp) => {
                        match resp.bytes().await {
                            Ok(bytes) => {
                                match Channel::read_from(Cursor::new(bytes)) {
                                    Ok(channel) => {
                                        let mut guids = seen_guids.lock().await;
                                        for item in channel.items() {
                                            let guid_str = item.guid().map(|g| g.value.to_string())
                                                .or_else(|| item.link().map(|l| l.to_string()))
                                                .unwrap_or_else(|| Uuid::new_v4().to_string());

                                            if !guids.contains(&guid_str) {
                                                guids.insert(guid_str.clone());
                                                
                                                // Create event
                                                // RSS dates are RFC-2822 usually.
                                                let pub_date = item.pub_date().and_then(|d| DateTime::parse_from_rfc2822(d).ok()).map(|d| d.with_timezone(&Utc)).unwrap_or(Utc::now());
                                                
                                                let title = item.title().unwrap_or("No Title").to_string();
                                                let content = item.description().unwrap_or("").to_string();
                                                
                                                // Analyze sentiment using local NLP
                                                let sentiment_score = sentiment_analyzer.analyze_news(&title, &content);
                                                
                                                let event = NewsEvent {
                                                    id: guid_str,
                                                    source: "RSS".to_string(), // Could parse channel title
                                                    title: title.clone(),
                                                    content,
                                                    url: item.link().map(|l| l.to_string()),
                                                    timestamp: pub_date,
                                                    sentiment_score: Some(sentiment_score),
                                                };

                                                if let Err(e) = tx.send(event).await {
                                                    error!("Failed to send RSS event: {}", e);
                                                    return; // Channel closed
                                                }
                                                
                                                let sentiment_label = if sentiment_score > 0.3 {
                                                    "📈 Bullish"
                                                } else if sentiment_score < -0.3 {
                                                    "📉 Bearish"
                                                } else {
                                                    "➖ Neutral"
                                                };
                                                info!("RSS New Item: {} [{}] (score: {:.2})", title, sentiment_label, sentiment_score);
                                            }
                                        }
                                    }
                                    Err(e) => error!("Failed to parse RSS feed: {}", e),
                                }
                            }
                            Err(e) => error!("Failed to read RSS bytes: {}", e),
                        }
                    }
                    Err(e) => error!("Failed to fetch RSS feed: {}", e),
                }
            }
        });

        Ok(rx)
    }
}
