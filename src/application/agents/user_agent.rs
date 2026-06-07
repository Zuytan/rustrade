pub mod activity;
pub mod metrics;

pub use activity::{ActivityEvent, ActivityEventType, EventSeverity, RightPanelTab};
pub use metrics::{StrategyInfo, TrendDirection};

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
use rust_decimal::Decimal;
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

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
    pub right_panel_tab: RightPanelTab,

    // Performance & Risk metrics (Dynamic)
    pub latency_ms: u64,
    pub risk_score: u8, // Risk appetite score (1-9)
    pub symbol_sentiments: std::collections::HashMap<String, Sentiment>,

    // Phase 4: Analytics State
    pub monte_carlo_result: Option<crate::domain::performance::monte_carlo::MonteCarloResult>,
    pub correlation_matrix: std::collections::HashMap<(String, String), f64>,

    // Dynamic Symbol Selection
    pub available_symbols: Vec<String>,
    pub active_symbols: Vec<String>,
    pub symbols_loading: bool,
    pub symbol_selector_state: crate::interfaces::settings_components::SymbolSelectorState,
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
            right_panel_tab: RightPanelTab::News,
            latency_ms: 12,                 // Default initial value
            risk_score: initial_risk_score, // Use the score from the loaded settings
            symbol_sentiments: std::collections::HashMap::new(),
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
                stop_loss: None,
                take_profit: None,
                correlation_id: Some(
                    crate::domain::trading::correlation::generate_correlation_id(),
                ),
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
                    if let Some(symbol) = sentiment.symbol.clone() {
                        self.symbol_sentiments.insert(symbol, sentiment);
                    }
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
}
