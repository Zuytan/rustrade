use crate::application::agents::analyst::AnalystCommand;
use crate::application::agents::sentinel::SentinelCommand;
use crate::application::risk_management::commands::RiskCommand;
use crate::application::system::SystemHandle;
use crate::domain::listener::NewsEvent;
use crate::domain::sentiment::Sentiment;
use crate::domain::trading::types::{Candle, TradeProposal};
use anyhow::Result;
use crossbeam_channel::Receiver;

/// Unified event type for the User Interface
#[derive(Clone, Debug)]
pub enum SystemEvent {
    Candle(Candle),
    Sentiment(Sentiment),
    News(NewsEvent),
    Log(String),
}

/// A client interface for interacting with the Trading System.
/// Abstracts away channel management and provides a clean API for the UI/UserAgent.
pub struct SystemClient {
    // Incoming Data
    log_rx: Receiver<String>,
    handle: SystemHandle,
}

impl SystemClient {
    pub fn new(handle: SystemHandle, log_rx: Receiver<String>) -> Self {
        Self { handle, log_rx }
    }

    /// Poll for the next available event from any channel.
    /// This is a non-blocking call that checks all channels in priority order.
    pub fn poll_next(&mut self) -> Option<SystemEvent> {
        // 1. Check Logs (High volume, simple string)
        if let Ok(msg) = self.log_rx.try_recv() {
            return Some(SystemEvent::Log(msg));
        }

        // 2. Check Candles (High priority for charts)
        // Broadcast channels use try_recv for non-blocking
        if let Ok(candle) = self.handle.candle_rx.try_recv() {
            return Some(SystemEvent::Candle(candle));
        }

        // 3. Check Sentiment
        if let Ok(sentiment) = self.handle.sentiment_rx.try_recv() {
            return Some(SystemEvent::Sentiment(sentiment));
        }

        // 4. Check News
        if let Ok(news) = self.handle.news_rx.try_recv() {
            return Some(SystemEvent::News(news));
        }

        None
    }

    // --- Command Methods ---

    pub fn submit_proposal(&self, proposal: TradeProposal) -> Result<()> {
        self.handle
            .proposal_tx
            .try_send(proposal)
            .map_err(|e| anyhow::anyhow!("Failed to send trade proposal: {}", e))
    }

    pub fn send_sentinel_command(&self, cmd: SentinelCommand) -> Result<()> {
        self.handle
            .sentinel_cmd_tx
            .try_send(cmd)
            .map_err(|e| anyhow::anyhow!("Failed to send sentinel command: {}", e))
    }

    pub fn send_risk_command(&self, cmd: RiskCommand) -> Result<()> {
        self.handle
            .risk_cmd_tx
            .try_send(cmd)
            .map_err(|e| anyhow::anyhow!("Failed to send risk command: {}", e))
    }

    pub fn send_analyst_command(&self, cmd: AnalystCommand) -> Result<()> {
        self.handle
            .analyst_cmd_tx
            .try_send(cmd)
            .map_err(|e| anyhow::anyhow!("Failed to send analyst command: {}", e))
    }

    // Accessors for shared state if needed
    pub fn portfolio(
        &self,
    ) -> std::sync::Arc<tokio::sync::RwLock<crate::domain::trading::portfolio::Portfolio>> {
        self.handle.portfolio.clone()
    }

    pub fn strategy_mode(&self) -> crate::domain::market::strategy_config::StrategyMode {
        self.handle.strategy_mode
    }

    pub fn risk_appetite(&self) -> Option<crate::domain::risk::risk_appetite::RiskAppetite> {
        self.handle.risk_appetite
    }
}
