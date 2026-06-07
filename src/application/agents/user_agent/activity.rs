use super::UserAgent;
use chrono::{DateTime, Utc};

/// Activity event type for the activity feed
#[derive(Clone, Debug)]
pub enum ActivityEventType {
    TradeExecuted,
    Signal,
    FilterBlock,
    StrategyChange,
    Alert,
    System,
}

/// Severity level for activity events
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventSeverity {
    Info,
    Warning,
    Error,
}

/// Activity event for the feed
#[derive(Clone, Debug)]
pub struct ActivityEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: ActivityEventType,
    pub message: String,
    pub severity: EventSeverity,
}

impl ActivityEvent {
    pub fn new(event_type: ActivityEventType, message: String, severity: EventSeverity) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            message,
            severity,
        }
    }
}

/// Tab options for the right panel in the Dashboard
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RightPanelTab {
    News,
    Activity,
}

impl UserAgent {
    /// Add an activity event to the feed (max 20 events)
    pub fn add_activity(
        &mut self,
        event_type: ActivityEventType,
        message: String,
        severity: EventSeverity,
    ) {
        self.activity_feed
            .push_front(ActivityEvent::new(event_type, message, severity));

        // Keep only last 20 events
        while self.activity_feed.len() > 20 {
            self.activity_feed.pop_back();
        }
    }

    /// Parse log messages to extract activity events
    pub fn parse_log_for_activity(&mut self, msg: &str) {
        // Check for order executions
        if msg.contains("Order") && (msg.contains("filled") || msg.contains("executed")) {
            if let Some(symbol) = self.extract_symbol_from_log(msg) {
                let event_msg = self
                    .i18n
                    .tf("activity_trade_executed", &[("symbol", &symbol)]);
                self.add_activity(
                    ActivityEventType::TradeExecuted,
                    event_msg,
                    EventSeverity::Info,
                );
            }
        }
        // Check for buy/sell signals
        else if msg.contains("SignalGenerator") {
            if (msg.contains("BUY") || msg.contains("SELL"))
                && let Some(symbol) = self.extract_symbol_from_log(msg)
            {
                let signal_type = if msg.contains("BUY") {
                    self.i18n.t("side_buy")
                } else {
                    self.i18n.t("side_sell")
                };
                let event_msg = self.i18n.tf(
                    "activity_signal",
                    &[("type", signal_type), ("symbol", &symbol)],
                );
                self.add_activity(ActivityEventType::Signal, event_msg, EventSeverity::Info);
            }
        }
        // Check for filter blocks
        else if msg.contains("REJECT") || msg.contains("blocked") || msg.contains("filtered") {
            if let Some(symbol) = self.extract_symbol_from_log(msg) {
                let reason = if msg.contains("RSI") {
                    self.i18n.t("filter_rsi")
                } else if msg.contains("cost") || msg.contains("Cost") {
                    self.i18n.t("filter_cost")
                } else if msg.contains("risk") || msg.contains("Risk") {
                    self.i18n.t("filter_risk")
                } else {
                    self.i18n.t("filter_generic")
                };
                let event_msg = self.i18n.tf(
                    "activity_blocked",
                    &[("symbol", &symbol), ("reason", reason)],
                );
                self.add_activity(
                    ActivityEventType::FilterBlock,
                    event_msg,
                    EventSeverity::Warning,
                );
            }
        }
        // Check for strategy changes
        else if msg.contains("Strategy") && msg.contains("changed") {
            self.add_activity(
                ActivityEventType::StrategyChange,
                self.i18n.t("activity_strategy_updated").to_string(),
                EventSeverity::Info,
            );
        }
        // Check for errors
        else if msg.contains("ERROR") {
            let short_msg = msg.chars().take(60).collect::<String>();
            self.add_activity(ActivityEventType::Alert, short_msg, EventSeverity::Error);
        }
        // Check for warnings
        else if msg.contains("WARN") && (msg.contains("Circuit") || msg.contains("limit")) {
            let short_msg = msg.chars().take(60).collect::<String>();
            self.add_activity(ActivityEventType::Alert, short_msg, EventSeverity::Warning);
        }
    }

    /// Extract symbol from log message (basic heuristic)
    pub fn extract_symbol_from_log(&self, msg: &str) -> Option<String> {
        // Try to find common symbol patterns (e.g., "BTC/USD", "AAPL")
        for word in msg.split_whitespace() {
            // Check if it looks like a symbol
            if word.contains("/") && word.len() <= 10 {
                // Crypto symbol like "BTC/USD"
                return Some(
                    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '/')
                        .to_string(),
                );
            } else if word.chars().all(|c| c.is_uppercase()) && word.len() >= 2 && word.len() <= 5 {
                // Stock symbol like "AAPL"
                return Some(word.to_string());
            }
        }
        None
    }
}
