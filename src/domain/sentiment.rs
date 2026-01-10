use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SentimentClassification {
    ExtremeFear,
    Fear,
    Neutral,
    Greed,
    ExtremeGreed,
}

impl fmt::Display for SentimentClassification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExtremeFear => write!(f, "Extreme Fear"),
            Self::Fear => write!(f, "Fear"),
            Self::Neutral => write!(f, "Neutral"),
            Self::Greed => write!(f, "Greed"),
            Self::ExtremeGreed => write!(f, "Extreme Greed"),
        }
    }
}

impl SentimentClassification {
    pub fn from_score(score: u8) -> Self {
        match score {
            0..=24 => Self::ExtremeFear,
            25..=44 => Self::Fear,
            45..=55 => Self::Neutral,
            56..=75 => Self::Greed,
            _ => Self::ExtremeGreed,
        }
    }

    pub fn color_hex(&self) -> &'static str {
        match self {
            Self::ExtremeFear => "#FF4500",  // Orange Red
            Self::Fear => "#FFA500",         // Orange
            Self::Neutral => "#808080",      // Gray
            Self::Greed => "#90EE90",        // Light Green
            Self::ExtremeGreed => "#008000", // Green
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sentiment {
    pub value: u8, // 0-100
    pub classification: SentimentClassification,
    pub timestamp: DateTime<Utc>,
    pub source: String,
}

#[async_trait]
pub trait SentimentProvider: Send + Sync {
    /// Fetch the current market sentiment
    async fn fetch_sentiment(&self) -> anyhow::Result<Sentiment>;
}
