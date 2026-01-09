//! Local NLP-based sentiment analysis using VADER
//!
//! This module provides sentiment analysis for news headlines and content
//! using the VADER (Valence Aware Dictionary and sEntiment Reasoner) algorithm,
//! enhanced with financial-specific keyword boosting.
//!
//! # Example
//! ```rust,ignore
//! use rustrade::infrastructure::news::sentiment_analyzer::SentimentAnalyzer;
//!
//! let analyzer = SentimentAnalyzer::new();
//! let score = analyzer.analyze("Bitcoin surges to new all-time high!");
//! assert!(score > 0.3); // Bullish
//! ```

use vader_sentiment::SentimentIntensityAnalyzer;

/// Financial keywords and their sentiment scores for boosting VADER analysis.
/// These help capture financial jargon that VADER's general lexicon may miss.
const BULLISH_KEYWORDS: &[(&str, f64)] = &[
    ("surge", 0.4),
    ("surges", 0.4),
    ("rally", 0.4),
    ("rallies", 0.4),
    ("soar", 0.5),
    ("soars", 0.5),
    ("skyrocket", 0.6),
    ("skyrockets", 0.6),
    ("bullish", 0.5),
    ("bull run", 0.5),
    ("all-time high", 0.5),
    ("ath", 0.4),
    ("breakout", 0.3),
    ("moon", 0.4),
    ("mooning", 0.5),
    ("pump", 0.3),
    ("adoption", 0.2),
    ("institutional", 0.2),
    ("partnership", 0.2),
    ("upgrade", 0.3),
    ("breakthrough", 0.4),
    ("record high", 0.4),
    ("massive gain", 0.4),
    ("opportunity", 0.2),
];

const BEARISH_KEYWORDS: &[(&str, f64)] = &[
    ("crash", -0.5),
    ("crashes", -0.5),
    ("plunge", -0.5),
    ("plunges", -0.5),
    ("dump", -0.4),
    ("dumps", -0.4),
    ("bearish", -0.5),
    ("collapse", -0.5),
    ("collapses", -0.5),
    ("lawsuit", -0.4),
    ("sec", -0.2),
    ("regulation", -0.2),
    ("ban", -0.4),
    ("hack", -0.5),
    ("hacked", -0.5),
    ("breach", -0.4),
    ("stolen", -0.5),
    ("scam", -0.6),
    ("fraud", -0.5),
    ("manipulation", -0.4),
    ("sell-off", -0.4),
    ("selloff", -0.4),
    ("panic", -0.4),
    ("fear", -0.3),
    ("devastating", -0.5),
];

/// A thread-safe sentiment analyzer using VADER algorithm with financial boosting.
///
/// VADER is specifically tuned for social media and news text.
/// This implementation adds financial keyword boosting to improve accuracy
/// for crypto and stock market news.
pub struct SentimentAnalyzer {
    analyzer: SentimentIntensityAnalyzer<'static>,
}

impl SentimentAnalyzer {
    /// Create a new sentiment analyzer instance.
    pub fn new() -> Self {
        Self {
            analyzer: SentimentIntensityAnalyzer::new(),
        }
    }

    /// Calculate financial keyword boost for the given text.
    fn financial_boost(&self, text: &str) -> f64 {
        let text_lower = text.to_lowercase();
        let mut boost = 0.0;

        for (keyword, score) in BULLISH_KEYWORDS {
            if text_lower.contains(keyword) {
                boost += score;
            }
        }

        for (keyword, score) in BEARISH_KEYWORDS {
            if text_lower.contains(keyword) {
                boost += score; // score is already negative
            }
        }

        boost
    }

    /// Analyze text and return a sentiment score between -1.0 and 1.0.
    ///
    /// - Score > 0.3: Bullish (positive sentiment)
    /// - Score < -0.3: Bearish (negative sentiment)
    /// - Score between -0.3 and 0.3: Neutral
    ///
    /// The analysis combines VADER's compound score with financial keyword boosting.
    pub fn analyze(&self, text: &str) -> f64 {
        if text.trim().is_empty() {
            return 0.0;
        }

        let scores = self.analyzer.polarity_scores(text);
        let vader_score = scores["compound"];
        let financial_boost = self.financial_boost(text);

        // Combine VADER score with financial boost, clamped to [-1, 1]
        let combined = vader_score + (financial_boost * 0.5);
        combined.clamp(-1.0, 1.0)
    }

    /// Analyze both title and content, combining scores with title weighted higher.
    ///
    /// Title weight: 70%, Content weight: 30%
    pub fn analyze_news(&self, title: &str, content: &str) -> f64 {
        let title_score = self.analyze(title);
        let content_score = self.analyze(content);

        // Weight title more heavily as it's typically more indicative
        (title_score * 0.7) + (content_score * 0.3)
    }
}

impl Default for SentimentAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bullish_headlines() {
        let analyzer = SentimentAnalyzer::new();

        let bullish_headlines = [
            "Bitcoin surges to new all-time high as institutional adoption grows",
            "Crypto market rallies 15% in massive bull run",
            "Dogecoin skyrockets after endorsement",
            "Ethereum breaks resistance, investors extremely bullish",
            "Major bank announces crypto trading platform - huge opportunity",
        ];

        for headline in bullish_headlines {
            let score = analyzer.analyze(headline);
            assert!(
                score > 0.0, // With financial boosting, should be positive
                "Expected bullish score for '{}', got {}",
                headline,
                score
            );
        }
    }

    #[test]
    fn test_bearish_headlines() {
        let analyzer = SentimentAnalyzer::new();

        let bearish_headlines = [
            "Bitcoin crashes 20% in devastating market collapse",
            "SEC files lawsuit against major crypto exchange",
            "Crypto exchange hacked, millions stolen in security breach",
            "Market panic as regulations threaten crypto industry",
            "Massive sell-off triggers fear and uncertainty",
        ];

        for headline in bearish_headlines {
            let score = analyzer.analyze(headline);
            assert!(
                score < 0.0, // With financial boosting, should be negative
                "Expected bearish score for '{}', got {}",
                headline,
                score
            );
        }
    }

    #[test]
    fn test_neutral_headlines() {
        let analyzer = SentimentAnalyzer::new();

        let neutral_headlines = [
            "Bitcoin trading volume remains steady",
            "Quarterly earnings report released",
            "Market closes unchanged from previous session",
        ];

        for headline in neutral_headlines {
            let score = analyzer.analyze(headline);
            assert!(
                score.abs() < 0.5,
                "Expected neutral score for '{}', got {}",
                headline,
                score
            );
        }
    }

    #[test]
    fn test_empty_text() {
        let analyzer = SentimentAnalyzer::new();
        assert_eq!(analyzer.analyze(""), 0.0);
        assert_eq!(analyzer.analyze("   "), 0.0);
    }

    #[test]
    fn test_combined_news_analysis() {
        let analyzer = SentimentAnalyzer::new();

        // Very bullish title with neutral content
        let score = analyzer.analyze_news(
            "Bitcoin surges to record high!",
            "The cryptocurrency traded between various levels today.",
        );
        assert!(score > 0.0, "Combined score should be positive: {}", score);
    }

    #[test]
    fn test_financial_boost() {
        let analyzer = SentimentAnalyzer::new();

        // Test that financial keywords boost the score
        let generic_positive = analyzer.analyze("This is good news");
        let financial_positive = analyzer.analyze("This shows bullish momentum with a surge");

        assert!(
            financial_positive > generic_positive,
            "Financial boosting should increase positive scores"
        );
    }
}
