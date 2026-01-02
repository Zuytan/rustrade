use crate::application::agents::sentinel::SentinelCommand;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::OrderSide;
use crate::domain::trading::types::TradeProposal;
use crossbeam_channel::Receiver;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::debug;

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
    
    // Log filtering
    pub log_level_filter: Option<String>, // None = All, Some("INFO"), Some("WARN"), Some("ERROR"), Some("DEBUG")
}

#[derive(Clone)]
pub struct StrategyInfo {
    pub mode: String,
    pub fast_sma: f64,
    pub slow_sma: f64,
    pub last_signal: Option<String>,
}

impl UserAgent {
    pub fn new(
        log_rx: Receiver<String>,
        candle_rx: broadcast::Receiver<Candle>,
        sentinel_cmd_tx: mpsc::Sender<SentinelCommand>,
        proposal_tx: mpsc::Sender<TradeProposal>,
        portfolio: Arc<RwLock<Portfolio>>,
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
            log_level_filter: None, // Show all logs by default
        }
    }

    /// Process the current input text as a command
    pub fn process_input(&mut self) -> Option<String> {
        let input = self.input_text.trim().to_string();
        if input.is_empty() {
            return None;
        }

        self.chat_history.push(("User".to_string(), input.clone()));
        self.input_text.clear();

        // Simple Natural Language Parsing
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["stop"] | ["halt"] | ["panic"] => {
                let _ = self.sentinel_cmd_tx.try_send(SentinelCommand::Shutdown);
                Some("Sent SHUTDOWN command to Sentinel.".to_string())
            }
            ["status"] => {
                // In a real agent, we might query the system.
                // For now, we just print local state or rely on logs.
                Some("Requesting system status... (check logs)".to_string())
            }
            ["buy", symbol, quantity] => {
                self.handle_trade_command(symbol, quantity, OrderSide::Buy)
            }
            ["sell", symbol, quantity] => {
                self.handle_trade_command(symbol, quantity, OrderSide::Sell)
            }
            _ => Some(format!(
                "Unknown command: '{}'. Try 'buy AAPL 10', 'status', or 'stop'.",
                input
            )),
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
                reason: "User Manual Command".to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(), // i64
            };

            match self.proposal_tx.try_send(proposal) {
                Ok(_) => Some(format!("Sent {:?} proposal for {} {}", side, qty, symbol)),
                Err(e) => Some(format!("Failed to send proposal: {}", e)),
            }
        } else {
            Some(format!("Invalid quantity: {}", quantity_str))
        }
    }

    /// Update internal state from incoming logs
    pub fn update(&mut self) {
        // 1. Logs
        // Drain all pending logs
        while let Ok(msg) = self.log_rx.try_recv() {
            // Simple heuristic to extract "Sender" from log line if possible,
            // otherwise default to "System"
            // Log format assumed: "TIMESTAMP LEVEL TARGET: MESSAGE"
            // We'll just display the raw message for now, or parse it lightly.
            self.chat_history.push(("System".to_string(), msg));
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

            // Initialize strategy info if not present
            if !self.strategy_info.contains_key(&candle.symbol) {
                self.strategy_info.insert(
                    candle.symbol.clone(),
                    StrategyInfo {
                        mode: "DualSMA".to_string(),
                        fast_sma: 20.0,
                        slow_sma: 50.0,
                        last_signal: None,
                    },
                );
            }
        }
    }
}
