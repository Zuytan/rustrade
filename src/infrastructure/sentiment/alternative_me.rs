use crate::domain::sentiment::{Sentiment, SentimentClassification, SentimentProvider};
use anyhow::Context;
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Deserialize)]
struct AlternativeMeResponse {
    data: Vec<AlternativeMeData>,
    _name: Option<String>,
    _metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct AlternativeMeData {
    value: String,
    _value_classification: String,
    timestamp: String,
    _time_until_update: Option<String>,
}

pub struct AlternativeMeSentimentProvider {
    client: Client,
    url: String,
}

impl AlternativeMeSentimentProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            url: "https://api.alternative.me/fng/".to_string(),
        }
    }
}

#[async_trait]
impl SentimentProvider for AlternativeMeSentimentProvider {
    async fn fetch_sentiment(&self) -> anyhow::Result<Sentiment> {
        info!("Fetching sentiment from Alternative.me...");
        
        let response = self.client
            .get(&self.url)
            .send()
            .await
            .context("Failed to send request to Alternative.me")?;

        if !response.status().is_success() {
            anyhow::bail!("Alternative.me API returned status: {}", response.status());
        }

        let body: AlternativeMeResponse = response
            .json()
            .await
            .context("Failed to parse Alternative.me response")?;

        if let Some(data) = body.data.first() {
            let value: u8 = data.value.parse().context("Failed to parse sentiment value")?;
            let timestamp_secs: i64 = data.timestamp.parse().context("Failed to parse timestamp")?;
            let timestamp = Utc.timestamp_opt(timestamp_secs, 0).unwrap();
            
            // Re-classify based on our domain rules to ensure consistency
            let classification = SentimentClassification::from_score(value);

            let sentiment = Sentiment {
                value,
                classification,
                timestamp,
                source: "Alternative.me (Crypto Fear & Greed)".to_string(),
            };
            
            info!("Fetched Sentiment: {} ({}) from {}", value, classification, sentiment.timestamp);
            Ok(sentiment)
        } else {
            anyhow::bail!("No sentiment data found in response");
        }
    }
}
