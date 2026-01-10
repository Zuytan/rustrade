use crate::domain::ports::OrderUpdate;
use crate::domain::sentiment::Sentiment;
use crate::domain::trading::types::TradeProposal;

/// Command abstraction for RiskManager operations
///
/// This enum represents all possible commands that can be processed by the RiskManager.
/// Using the Command Pattern allows for better testability and separation of concerns.
#[derive(Debug)]
pub enum RiskCommand {
    /// Process an order status update from the broker
    OrderUpdate(OrderUpdate),

    /// Periodic portfolio valuation tick (triggered by interval timer)
    ValuationTick,

    /// Refresh portfolio state from broker (triggered by interval timer)
    RefreshPortfolio,

    /// Validate and potentially execute a trade proposal from Analyst
    ProcessProposal(TradeProposal),

    /// Update market sentiment state
    UpdateSentiment(Sentiment),

    /// Update risk configuration dynamically
    UpdateConfig(Box<crate::application::risk_management::risk_manager::RiskConfig>),

    /// Manually trigger circuit breaker (Testing/Panic)
    CircuitBreakerTrigger,
}

impl RiskCommand {
    /// Returns the command name for logging purposes
    pub fn name(&self) -> &'static str {
        match self {
            Self::OrderUpdate(_) => "OrderUpdate",
            Self::ValuationTick => "ValuationTick",
            Self::RefreshPortfolio => "RefreshPortfolio",
            Self::ProcessProposal(_) => "ProcessProposal",
            Self::UpdateSentiment(_) => "UpdateSentiment",
            Self::UpdateConfig(_) => "UpdateConfig",
            Self::CircuitBreakerTrigger => "CircuitBreakerTrigger",
        }
    }
}
