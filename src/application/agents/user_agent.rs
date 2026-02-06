use crate::application::agents::analyst::AnalystCommand;
use crate::application::agents::sentinel::SentinelCommand;
use crate::application::client::{SystemClient, SystemEvent};
use crate::application::risk_management::commands::RiskCommand;
use crate::domain::listener::NewsEvent;
use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::sentiment::Sentiment;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::OrderSide;
use crate::domain::trading::types::TradeProposal;
use crate::infrastructure::i18n::I18nService;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

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
    pub client: SystemClient,
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

    // News feed (max 10 events)
    pub news_events: VecDeque<NewsEvent>,

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
    pub risk_score: u8, // Risk appetite score (1-9)
    pub market_sentiment: Option<Sentiment>,

    // Phase 4: Analytics State
    pub monte_carlo_result: Option<crate::domain::performance::monte_carlo::MonteCarloResult>,
    pub correlation_matrix: std::collections::HashMap<(String, String), f64>,

    // Dynamic Symbol Selection
    pub available_symbols: Vec<String>,
    pub active_symbols: Vec<String>,
    pub symbols_loading: bool,
    pub symbol_selector_state: crate::interfaces::settings_components::SymbolSelectorState,
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

pub struct UserAgentConfig {
    pub strategy_mode: StrategyMode,
    pub risk_appetite: Option<crate::domain::risk::risk_appetite::RiskAppetite>,
}

impl UserAgent {
    pub fn new(
        client: SystemClient,
        portfolio: Arc<RwLock<Portfolio>>,
        config: UserAgentConfig,
    ) -> Self {
        // Initialize I18n and SettingsPanel first
        let i18n = I18nService::new();
        let settings_panel = crate::interfaces::ui_components::SettingsPanel::new();

        // --- Sync Persisted Settings to Agents ---
        // Since SettingsPanel loads from disk on ::new(), we send those values
        // to the backend agents (RiskManager, Analyst) to ensure they are insync on startup.
        let risk_config = settings_panel.to_risk_config();
        if let Err(e) = client.send_risk_command(RiskCommand::UpdateConfig(Box::new(risk_config))) {
            error!("Failed to sync risk settings on startup: {}", e);
        }

        let analyst_config = settings_panel.to_analyst_config();
        if let Err(e) =
            client.send_analyst_command(AnalystCommand::UpdateConfig(Box::new(analyst_config)))
        {
            error!("Failed to sync analyst settings on startup: {}", e);
        } else {
            info!("Successfully synced persisted settings to Analyst and RiskManager");
        }

        let initial_risk_score = settings_panel.risk_score;

        Self {
            client,
            portfolio,
            chat_history: Vec::new(),
            input_text: String::new(),
            is_focused: true,
            market_data: std::collections::HashMap::new(),
            selected_chart_tab: None,
            strategy_info: std::collections::HashMap::new(),
            strategy_mode: config.strategy_mode,
            log_level_filter: None, // Show all logs by default
            activity_feed: VecDeque::new(),
            news_events: VecDeque::new(),
            logs_collapsed: true, // Collapsed by default
            total_trades: 0,
            winning_trades: 0,
            i18n,
            settings_panel,
            current_view: crate::interfaces::ui_components::DashboardView::Dashboard,
            latency_ms: 12,                 // Default initial value
            risk_score: initial_risk_score, // Use the score from the loaded settings
            market_sentiment: None,
            monte_carlo_result: None,
            correlation_matrix: std::collections::HashMap::new(),
            // Dynamic Symbol Selection
            available_symbols: Vec::new(),
            active_symbols: Vec::new(),
            symbols_loading: false,
            symbol_selector_state: crate::interfaces::settings_components::SymbolSelectorState::new(
            ),
        }
    }

    /// Process the current input text as a command
    pub fn process_input(&mut self) -> Option<String> {
        let input = self.input_text.trim().to_string();
        if input.is_empty() {
            return None;
        }

        self.chat_history
            .push((self.i18n.t("sender_user").to_string(), input.clone()));
        self.input_text.clear();

        // Simple Natural Language Parsing
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["stop"] | ["halt"] | ["panic"] => {
                let _ = self.client.send_sentinel_command(SentinelCommand::Shutdown);
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

            match self.client.submit_proposal(proposal) {
                Ok(_) => Some(
                    self.i18n.tf(
                        "cmd_proposal_sent",
                        &[
                            (
                                "side",
                                self.i18n
                                    .t(&format!("side_{}", side.to_string().to_lowercase())),
                            ),
                            ("qty", &qty.to_string()),
                            ("symbol", symbol),
                        ],
                    ),
                ),
                Err(e) => Some(
                    self.i18n
                        .tf("cmd_proposal_failed", &[("error", &e.to_string())]),
                ),
            }
        } else {
            Some(self.i18n.tf("cmd_invalid_qty", &[("qty", quantity_str)]))
        }
    }

    /// Update internal state from incoming events
    pub fn update(&mut self) {
        // Poll all events from the client
        while let Some(event) = self.client.poll_next() {
            match event {
                SystemEvent::Log(msg) => {
                    // Parse logs for activity events
                    self.parse_log_for_activity(&msg);

                    // Extract signal information from SignalGenerator logs
                    if msg.contains("SignalGenerator")
                        && msg.contains(": ")
                        && let Some(signal_part) = msg.split("SignalGenerator").nth(1)
                    {
                        // Extract symbol and reason
                        if let Some(content) = signal_part.split(" - ").nth(1) {
                            // Find the symbol (between]: and -)
                            if let Some(symbol_section) = signal_part.split("]: ").nth(1)
                                && let Some(symbol) = symbol_section.split(" - ").next()
                            {
                                // Update strategy info with the signal reason
                                if let Some(info) = self.strategy_info.get_mut(symbol) {
                                    info.last_signal = Some(content.trim().to_string());
                                }
                            }
                        }
                    }

                    // Add to chat history
                    self.chat_history
                        .push((self.i18n.t("sender_system").to_string(), msg));
                }
                SystemEvent::Sentiment(sentiment) => {
                    debug!(
                        "UserAgent: Received new sentiment: {} ({})",
                        sentiment.value, sentiment.classification
                    );
                    self.market_sentiment = Some(sentiment);
                }
                SystemEvent::News(news) => {
                    debug!(
                        "UserAgent: Received news event: {} - {}",
                        news.source, news.title
                    );
                    self.news_events.push_front(news);
                    // Keep only last 10 news events
                    while self.news_events.len() > 10 {
                        self.news_events.pop_back();
                    }
                }
                SystemEvent::Candle(candle) => {
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

                    // Calculate SMAs and trend for this symbol
                    let (fast_sma_value, slow_sma_value, trend) =
                        self.calculate_trend(&candle.symbol);

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
        }

        // Keep history manageable (outside the loop to do it once per update tick)
        if self.chat_history.len() > 1000 {
            self.chat_history.drain(0..100);
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

    /// Calculate average win and loss percentages based on trade history
    /// Returns (avg_win_pct, avg_loss_pct) as decimals (e.g. 0.05 for 5%)
    pub fn calculate_trade_statistics(&self) -> (f64, f64) {
        if let Ok(pf) = self.portfolio.try_read() {
            let mut total_win_pct = 0.0;
            let mut total_loss_pct = 0.0;
            let mut win_count = 0;
            let mut loss_count = 0;

            for trade in &pf.trade_history {
                if trade.exit_price.is_none() {
                    continue;
                }

                let entry_val = trade.entry_price.to_f64().unwrap_or(0.0);
                if entry_val == 0.0 {
                    continue;
                }

                // PnL percentage relative to entry price
                // For LONG: (exit - entry) / entry
                // For SHORT: (entry - exit) / entry
                // This is equivalent to trade.pnl / (entry * quantity), but per unit is simpler
                let pnl_per_unit = if trade.side == OrderSide::Buy {
                    trade.exit_price.and_then(|p| p.to_f64()).unwrap_or(0.0) - entry_val
                } else {
                    entry_val - trade.exit_price.and_then(|p| p.to_f64()).unwrap_or(0.0)
                };

                let pct = pnl_per_unit / entry_val;

                if pct > 0.0 {
                    total_win_pct += pct;
                    win_count += 1;
                } else {
                    total_loss_pct += pct.abs();
                    loss_count += 1;
                }
            }

            let avg_win = if win_count > 0 {
                total_win_pct / win_count as f64
            } else {
                0.0
            };
            let avg_loss = if loss_count > 0 {
                total_loss_pct / loss_count as f64
            } else {
                0.0
            };

            // Return safe defaults if no history yet to avoid flat lines in Monte Carlo
            let safe_win = if avg_win == 0.0 { 0.02 } else { avg_win };
            let safe_loss = if avg_loss == 0.0 { 0.015 } else { avg_loss };

            (safe_win, safe_loss)
        } else {
            (0.02, 0.015)
        }
    }

    /// Parse log messages to extract activity events
    fn parse_log_for_activity(&mut self, msg: &str) {
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
    /// Calculate full performance metrics from portfolio history
    pub fn get_performance_metrics(
        &self,
    ) -> crate::domain::performance::metrics::PerformanceMetrics {
        if let Ok(pf) = self.portfolio.try_read() {
            // Use starting cash as initial equity
            let initial_equity = if pf.starting_cash > Decimal::ZERO {
                pf.starting_cash
            } else {
                dec!(10000) // Default fallback if not set
            };

            // Calculate period in days (from first trade to now, or 1 day min)
            let start_ts = pf
                .trade_history
                .first()
                .map(|t| t.entry_timestamp)
                .unwrap_or(chrono::Utc::now().timestamp_millis());
            let end_ts = chrono::Utc::now().timestamp_millis();
            let period_days = ((end_ts - start_ts) as f64 / (1000.0 * 3600.0 * 24.0)).max(1.0);

            crate::domain::performance::metrics::PerformanceMetrics::calculate(
                &pf.trade_history,
                initial_equity,
                pf.total_equity(&HashMap::new()), // Approximation without live prices
                period_days,
            )
        } else {
            crate::domain::performance::metrics::PerformanceMetrics::default()
        }
    }

    /// Generate equity curve points for plotting [timestamp, equity]
    pub fn get_equity_curve_points(&self) -> Vec<[f64; 2]> {
        if let Ok(pf) = self.portfolio.try_read() {
            let initial_equity = if pf.starting_cash > Decimal::ZERO {
                pf.starting_cash
            } else {
                dec!(10000)
            };

            let mut curve = Vec::new();
            // Start point
            let start_ts = pf
                .trade_history
                .first()
                .map(|t| t.entry_timestamp)
                .unwrap_or(chrono::Utc::now().timestamp_millis() - 86400000);

            curve.push([
                start_ts as f64 / 1000.0,
                initial_equity.to_f64().unwrap_or(0.0),
            ]);

            let mut current_equity = initial_equity;
            for trade in &pf.trade_history {
                current_equity += trade.pnl;
                if let Some(exit_ts) = trade.exit_timestamp {
                    curve.push([
                        exit_ts as f64 / 1000.0,
                        current_equity.to_f64().unwrap_or(0.0),
                    ]);
                }
            }

            // Add current point
            let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
            curve.push([now, current_equity.to_f64().unwrap_or(0.0)]);

            curve
        } else {
            vec![]
        }
    }
}
