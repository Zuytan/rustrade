use crate::application::agents::sentinel::SentinelCommand;
use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::OrderSide;
use crate::domain::trading::types::TradeProposal;
use crate::domain::ui::I18nService;
use chrono::{DateTime, Utc};
use crossbeam_channel::Receiver;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::debug;

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

pub struct UserAgent {
    pub log_rx: Receiver<String>,
    pub candle_rx: broadcast::Receiver<Candle>,
    pub sentinel_cmd_tx: mpsc::Sender<SentinelCommand>,
    pub proposal_tx: mpsc::Sender<TradeProposal>,
    pub portfolio: Arc<RwLock<Portfolio>>,

    // UI State
    pub chat_history: Vec<(String, String)>, // (Sender, Message)
    pub input_text: String,
    pub is_focused: bool,
    pub market_data: std::collections::HashMap<String, Vec<Candle>>, // Store history
    pub selected_chart_tab: Option<String>, // Currently selected symbol for chart
    pub strategy_info: std::collections::HashMap<String, StrategyInfo>, // Strategy per symbol
    pub strategy_mode: StrategyMode,        // Added: Actual strategy mode from config

    // Log filtering
    pub log_level_filter: Option<String>, // None = All, Some("INFO"), Some("WARN"), Some("ERROR"), Some("DEBUG")

    // Activity feed (max 20 events)
    pub activity_feed: VecDeque<ActivityEvent>,

    // UI state
    pub logs_collapsed: bool,

    // Portfolio metrics tracking
    pub total_trades: usize,
    pub winning_trades: usize,

    // Internationalization
    pub i18n: I18nService,

    // Settings panel state
    pub settings_panel: crate::interfaces::ui_components::SettingsPanel,
    
    // Dashboard Navigation State
    pub current_view: crate::interfaces::ui_components::DashboardView,

    // Performance & Risk metrics (Dynamic)
    pub latency_ms: u64,
    pub risk_score: u8,  // Risk appetite score (1-9)
}

/// Direction of the market trend for a symbol
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrendDirection {
    Bullish,
    Bearish,
    Sideways,
}

impl TrendDirection {
    /// Returns an emoji representation of the trend
    pub fn emoji(&self) -> &'static str {
        match self {
            TrendDirection::Bullish => "📈",
            TrendDirection::Bearish => "📉",
            TrendDirection::Sideways => "→",
        }
    }
}

#[derive(Clone)]
pub struct StrategyInfo {
    pub mode: String,
    pub fast_sma: f64,
    pub slow_sma: f64,
    pub last_signal: Option<String>,
    pub trend: TrendDirection,
    pub current_price: Decimal,
}

impl UserAgent {
    pub fn new(
        log_rx: Receiver<String>,
        candle_rx: broadcast::Receiver<Candle>,
        sentinel_cmd_tx: mpsc::Sender<SentinelCommand>,
        proposal_tx: mpsc::Sender<TradeProposal>,
        portfolio: Arc<RwLock<Portfolio>>,
        strategy_mode: StrategyMode,
        risk_appetite: Option<crate::domain::risk::risk_appetite::RiskAppetite>,
    ) -> Self {
        Self {
            log_rx,
            candle_rx,
            sentinel_cmd_tx,
            proposal_tx,
            portfolio,
            chat_history: Vec::new(),
            input_text: String::new(),
            is_focused: true,
            market_data: std::collections::HashMap::new(),
            selected_chart_tab: None,
            strategy_info: std::collections::HashMap::new(),
            strategy_mode,
            log_level_filter: None, // Show all logs by default
            activity_feed: VecDeque::new(),
            logs_collapsed: true, // Collapsed by default
            total_trades: 0,
            winning_trades: 0,
            i18n: I18nService::new(), // Initialize i18n service
            settings_panel: crate::interfaces::ui_components::SettingsPanel::new(),
            current_view: crate::interfaces::ui_components::DashboardView::Dashboard,
            latency_ms: 12,    // Default initial value
            risk_score: risk_appetite.map(|r| r.score()).unwrap_or(5),  // Use real risk score or default to 5 (balanced)
        }
    }

    /// Process the current input text as a command
    pub fn process_input(&mut self) -> Option<String> {
        let input = self.input_text.trim().to_string();
        if input.is_empty() {
            return None;
        }

        self.chat_history.push((self.i18n.t("sender_user").to_string(), input.clone()));
        self.input_text.clear();

        // Simple Natural Language Parsing
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["stop"] | ["halt"] | ["panic"] => {
                let _ = self.sentinel_cmd_tx.try_send(SentinelCommand::Shutdown);
                Some(self.i18n.t("cmd_shutdown_sent").to_string())
            }
            ["status"] => {
                // In a real agent, we might query the system.
                // For now, we just print local state or rely on logs.
                Some(self.i18n.t("cmd_status_request").to_string())
            }
            ["buy", symbol, quantity] => {
                self.handle_trade_command(symbol, quantity, OrderSide::Buy)
            }
            ["sell", symbol, quantity] => {
                self.handle_trade_command(symbol, quantity, OrderSide::Sell)
            }
            _ => Some(self.i18n.tf("cmd_unknown", &[("input", &input)])),
        }
    }

    fn handle_trade_command(
        &self,
        symbol: &str,
        quantity_str: &str,
        side: OrderSide,
    ) -> Option<String> {
        if let Ok(qty) = Decimal::from_str(quantity_str) {
            let proposal = TradeProposal {
                symbol: symbol.to_uppercase(),
                quantity: qty,
                side,
                order_type: crate::domain::trading::types::OrderType::Market, // Default to Market
                price: Decimal::ZERO, // Ignored for Market orders
                reason: self.i18n.t("activity_user_command").to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(), // i64
            };

            match self.proposal_tx.try_send(proposal) {
                Ok(_) => Some(self.i18n.tf("cmd_proposal_sent", &[
                    ("side", &self.i18n.t(&format!("side_{}", side.to_string().to_lowercase())).to_string()),
                    ("qty", &qty.to_string()),
                    ("symbol", symbol)
                ])),
                Err(e) => Some(self.i18n.tf("cmd_proposal_failed", &[("error", &e.to_string())])),
            }
        } else {
            Some(self.i18n.tf("cmd_invalid_qty", &[("qty", quantity_str)]))
        }
    }

    /// Update internal state from incoming logs
    pub fn update(&mut self) {
        // 1. Logs
        // Drain all pending logs
        while let Ok(msg) = self.log_rx.try_recv() {
            // Parse logs for activity events
            self.parse_log_for_activity(&msg);

            // Extract signal information from SignalGenerator logs
            // Format: "SignalGenerator [StrategyName]: SYMBOL - REASON"
            if msg.contains("SignalGenerator") && msg.contains(": ") {
                if let Some(signal_part) = msg.split("SignalGenerator").nth(1) {
                    // Extract symbol and reason
                    if let Some(content) = signal_part.split(" - ").nth(1) {
                        // Find the symbol (between]: and -)
                        if let Some(symbol_section) = signal_part.split("]: ").nth(1) {
                            if let Some(symbol) = symbol_section.split(" - ").next() {
                                // Update strategy info with the signal reason
                                if let Some(info) = self.strategy_info.get_mut(symbol) {
                                    info.last_signal = Some(content.trim().to_string());
                                }
                            }
                        }
                    }
                }
            }

            // Simple heuristic to extract "Sender" from log line if possible,
            // otherwise default to "System"
            // Log format assumed: "TIMESTAMP LEVEL TARGET: MESSAGE"
            // We'll just display the raw message for now, or parse it lightly.
            self.chat_history.push((self.i18n.t("sender_system").to_string(), msg));
        }

        // Keep history manageable
        if self.chat_history.len() > 1000 {
            self.chat_history.drain(0..100);
        }

        // 2. Candles
        while let Ok(candle) = self.candle_rx.try_recv() {
            debug!(
                "UserAgent: Received candle for {} at price {}",
                candle.symbol, candle.close
            );
            let entry = self.market_data.entry(candle.symbol.clone()).or_default();
            entry.push(candle.clone());
            // Keep last 100 candles
            if entry.len() > 100 {
                entry.remove(0);
            }
            debug!(
                "UserAgent: Market data now has {} candles for {}",
                entry.len(),
                entry.last().unwrap().symbol
            );

            // Calculate SMAs and trend for this symbol
            let (fast_sma_value, slow_sma_value, trend) = self.calculate_trend(&candle.symbol);

            // Initialize or update strategy info
            if let Some(info) = self.strategy_info.get_mut(&candle.symbol) {
                // Update existing entry
                info.fast_sma = fast_sma_value;
                info.slow_sma = slow_sma_value;
                info.trend = trend;
                info.current_price = candle.close;
            } else {
                // Create new entry
                self.strategy_info.insert(
                    candle.symbol.clone(),
                    StrategyInfo {
                        mode: self.strategy_mode.to_string(),
                        fast_sma: fast_sma_value,
                        slow_sma: slow_sma_value,
                        last_signal: None,
                        trend,
                        current_price: candle.close,
                    },
                );
            }
        }
    }

    /// Calculate SMAs and trend direction for a symbol
    fn calculate_trend(&self, symbol: &str) -> (f64, f64, TrendDirection) {
        let fast_period = 20;
        let slow_period = 50;

        let candles = match self.market_data.get(symbol) {
            Some(c) => c,
            None => return (0.0, 0.0, TrendDirection::Sideways),
        };

        // Calculate fast SMA
        let fast_sma = if candles.len() >= fast_period {
            let sum: f64 = candles[candles.len() - fast_period..]
                .iter()
                .map(|c| c.close.to_f64().unwrap_or(0.0))
                .sum();
            sum / fast_period as f64
        } else {
            0.0
        };

        // Calculate slow SMA
        let slow_sma = if candles.len() >= slow_period {
            let sum: f64 = candles[candles.len() - slow_period..]
                .iter()
                .map(|c| c.close.to_f64().unwrap_or(0.0))
                .sum();
            sum / slow_period as f64
        } else {
            0.0
        };

        // Determine trend based on SMA relationship
        let trend = if fast_sma == 0.0 || slow_sma == 0.0 {
            TrendDirection::Sideways
        } else {
            let diff_pct = (fast_sma - slow_sma) / slow_sma * 100.0;
            if diff_pct > 0.5 {
                TrendDirection::Bullish
            } else if diff_pct < -0.5 {
                TrendDirection::Bearish
            } else {
                TrendDirection::Sideways
            }
        };

        (fast_sma, slow_sma, trend)
    }

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

    /// Calculate total portfolio value (cash + positions)
    pub fn calculate_total_value(&self) -> Decimal {
        if let Ok(pf) = self.portfolio.try_read() {
            let mut position_value = Decimal::ZERO;
            for (symbol, pos) in &pf.positions {
                // Get current price from strategy_info
                let current_price = self
                    .strategy_info
                    .get(symbol)
                    .map(|info| info.current_price)
                    .unwrap_or(pos.average_price);
                position_value += pos.quantity * current_price;
            }
            pf.cash + position_value
        } else {
            Decimal::ZERO
        }
    }

    /// Calculate win rate as a percentage
    pub fn calculate_win_rate(&self) -> f64 {
        if self.total_trades == 0 {
            0.0
        } else {
            (self.winning_trades as f64 / self.total_trades as f64) * 100.0
        }
    }

    /// Parse log messages to extract activity events
    fn parse_log_for_activity(&mut self, msg: &str) {
        // Check for order executions
        if msg.contains("Order") && (msg.contains("filled") || msg.contains("executed")) {
            if let Some(symbol) = self.extract_symbol_from_log(msg) {
                let event_msg = self.i18n.tf("activity_trade_executed", &[("symbol", &symbol)]);
                self.add_activity(
                    ActivityEventType::TradeExecuted,
                    event_msg,
                    EventSeverity::Info,
                );
            }
        }
        // Check for buy/sell signals
        else if msg.contains("SignalGenerator") {
            if msg.contains("BUY") || msg.contains("SELL") {
                if let Some(symbol) = self.extract_symbol_from_log(msg) {
                    let signal_type = if msg.contains("BUY") { 
                        self.i18n.t("side_buy") 
                    } else { 
                        self.i18n.t("side_sell") 
                    };
                    let event_msg = self.i18n.tf("activity_signal", &[
                        ("type", &signal_type.to_string()),
                        ("symbol", &symbol)
                    ]);
                    self.add_activity(ActivityEventType::Signal, event_msg, EventSeverity::Info);
                }
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
                let event_msg = self.i18n.tf("activity_blocked", &[
                    ("symbol", &symbol),
                    ("reason", &reason.to_string())
                ]);
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
    fn extract_symbol_from_log(&self, msg: &str) -> Option<String> {
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
