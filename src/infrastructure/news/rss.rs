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
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info};
use uuid::Uuid;

pub struct RssNewsService {
    urls: Arc<RwLock<Vec<String>>>,
    client: Client,
    seen_guids: Arc<Mutex<HashSet<String>>>,
    poll_interval_seconds: u64,
    sentiment_analyzer: Arc<SentimentAnalyzer>,
    notify_update: Arc<tokio::sync::Notify>,
}

impl RssNewsService {
    pub fn new(urls: Arc<RwLock<Vec<String>>>, poll_interval_seconds: u64) -> Self {
        Self {
            urls,
            client: Client::new(),
            seen_guids: Arc::new(Mutex::new(HashSet::new())),
            poll_interval_seconds,
            sentiment_analyzer: Arc::new(SentimentAnalyzer::new()),
            notify_update: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

#[async_trait]
impl NewsDataService for RssNewsService {
    async fn subscribe_news(&self) -> Result<Receiver<NewsEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let urls_lock = self.urls.clone();
        let client = self.client.clone();
        let seen_guids = self.seen_guids.clone();
        let interval_sec = self.poll_interval_seconds;
        let sentiment_analyzer = self.sentiment_analyzer.clone();
        let notify_update = self.notify_update.clone();

        tokio::spawn(async move {
            info!("Starting RSS News Poller (dynamic URLs, NLP sentiment)");

            loop {
                let current_urls = urls_lock.read().await.clone();
                for url in current_urls {
                    debug!("Polling RSS feed: {}", url);
                    let content_result = client.get(&url).send().await;

                    match content_result {
                        Ok(resp) => {
                            match resp.bytes().await {
                                Ok(bytes) => {
                                    match Channel::read_from(Cursor::new(bytes)) {
                                        Ok(channel) => {
                                            let mut guids = seen_guids.lock().await;

                                            // Sort items by pub date (newest first) to ensure we emit the most recent ones first
                                            let mut items = channel.items().to_vec();
                                            items.sort_by(|a, b| {
                                                let date_a = a
                                                    .pub_date()
                                                    .and_then(|d| {
                                                        DateTime::parse_from_rfc2822(d).ok()
                                                    })
                                                    .unwrap_or_default();
                                                let date_b = b
                                                    .pub_date()
                                                    .and_then(|d| {
                                                        DateTime::parse_from_rfc2822(d).ok()
                                                    })
                                                    .unwrap_or_default();
                                                date_b.cmp(&date_a)
                                            });

                                            // Determine if this is the first run for this feed by checking if guids is empty
                                            // We only emit a max of 3 items per feed on first run to avoid barraging the system
                                            let is_first_run = guids.is_empty();
                                            let mut emitted_count = 0;

                                            for item in items {
                                                let guid_str = item
                                                    .guid()
                                                    .map(|g| g.value.to_string())
                                                    .or_else(|| item.link().map(|l| l.to_string()))
                                                    .unwrap_or_else(|| Uuid::new_v4().to_string());

                                                if is_first_run && emitted_count >= 3 {
                                                    // First run cap reached. Mark as seen but do not analyze/emit.
                                                    guids.insert(guid_str);
                                                    continue;
                                                }

                                                if !guids.contains(&guid_str) {
                                                    guids.insert(guid_str.clone());

                                                    let pub_date = item
                                                        .pub_date()
                                                        .and_then(|d| {
                                                            DateTime::parse_from_rfc2822(d).ok()
                                                        })
                                                        .map(|d| d.with_timezone(&Utc))
                                                        .unwrap_or(Utc::now());

                                                    let title = item
                                                        .title()
                                                        .unwrap_or("No Title")
                                                        .to_string();
                                                    let content = item
                                                        .description()
                                                        .unwrap_or("")
                                                        .to_string();

                                                    let sentiment_score = sentiment_analyzer
                                                        .analyze_news(&title, &content);

                                                    let event = NewsEvent {
                                                        id: guid_str.clone(),
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
                                                    info!(
                                                        "RSS New Item: {} [{}] (score: {:.2})",
                                                        title, sentiment_label, sentiment_score
                                                    );
                                                    emitted_count += 1;
                                                }
                                            }
                                        }
                                        Err(e) => error!("Failed to parse RSS feed {}: {}", url, e),
                                    }
                                }
                                Err(e) => error!("Failed to read RSS bytes {}: {}", url, e),
                            }
                        }
                        Err(e) => error!("Failed to fetch RSS feed {}: {}", url, e),
                    }
                }

                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(interval_sec)) => {}
                    _ = notify_update.notified() => {
                        info!("RssNewsService: Woken up early by URL update");
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn update_urls(&self, urls: Vec<String>) -> Result<()> {
        info!(
            "RssNewsService: Updating RSS URLs dynamically to {:?}",
            urls
        );
        let mut lock = self.urls.write().await;
        *lock = urls;
        self.notify_update.notify_one();
        Ok(())
    }
}
