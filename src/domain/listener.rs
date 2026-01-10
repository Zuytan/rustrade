use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsEvent {
    pub id: String,
    pub source: String,
    pub title: String,
    pub content: String,
    pub url: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub sentiment_score: Option<f64>, // -1.0 to 1.0, provided by source if available
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewsSentiment {
    Bullish,
    Bearish,
    Neutral,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsSignal {
    pub symbol: String,
    pub sentiment: NewsSentiment,
    pub headline: String,
    pub source: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListenerAction {
    NotifyAnalyst(NewsSentiment), // New action type
    // Deprecated for now, or keep for backward compat until full migration
    BuyImmediate,
    SellImmediate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenerRule {
    pub id: String,
    pub keywords: Vec<String>, // All keywords must match (AND logic for simplicity first, or OR?) -> Let's say ALL for now to be specific.
    pub target_symbol: String, // e.g. "DOGE/USD"
    pub action: ListenerAction,
    pub active: bool,
}

impl ListenerRule {
    pub fn matches(&self, text: &str) -> bool {
        let text_lower = text.to_lowercase();
        self.keywords
            .iter()
            .all(|k| text_lower.contains(&k.to_lowercase()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenerConfig {
    pub rules: Vec<ListenerRule>,
    pub poll_interval_seconds: u64,
}
