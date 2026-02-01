//! News Signal Handler
//!
//! Processes news signals and generates appropriate trading actions
//! based on sentiment analysis and technical filters.
//!
//! Extracted from [`Analyst`] to reduce module complexity.

use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::listener::NewsSignal;
use crate::domain::ports::ExecutionService;
use crate::domain::trading::symbol_context::SymbolContext;
use crate::domain::trading::types::{OrderSide, TradeProposal};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{info, warn};

/// Result of news signal processing
pub enum NewsAction {
    /// Buy proposal generated
    Buy(TradeProposal),
    /// Tightened trailing stop (no proposal)
    TightenStop,
    /// Panic sell proposal generated  
    PanicSell(TradeProposal),
    /// Signal rejected (filtered out)
    Rejected(String),
    /// No action taken
    NoAction,
}

use rust_decimal_macros::dec;

use crate::application::agents::signal_processor::SignalProcessor;

pub struct NewsHandler {
    signal_processor: SignalProcessor,
}

impl NewsHandler {
    pub fn new(signal_processor: SignalProcessor) -> Self {
        Self { signal_processor }
    }

    /// Processes a bullish news signal with technical filters.
    pub async fn process_bullish_news(
        &self,
        config: &AnalystConfig,
        execution_service: &Arc<dyn ExecutionService>,
        signal: &NewsSignal,
        context: &SymbolContext,
        price: Decimal,
        timestamp: i64,
    ) -> NewsAction {
        let sma_50 = context.last_features.sma_50.unwrap_or(Decimal::ZERO);
        let rsi = context.last_features.rsi.unwrap_or(dec!(50.0));

        info!(
            "NewsHandler: Analyzing BULLISH news for {}. Price: {}, SMA50: {}, RSI: {}",
            signal.symbol, price, sma_50, rsi
        );

        // 1. Trend Filter: Avoid buying falling knives
        if price < sma_50 {
            let reason = format!("Price ({}) below SMA50 ({}) - Bearish Trend", price, sma_50);
            warn!(
                "NewsHandler: REJECTED Bullish News for {}. {}",
                signal.symbol, reason
            );
            return NewsAction::Rejected(reason);
        }

        // 2. Overbought Filter: Avoid FOMO
        if rsi > dec!(75.0) {
            let reason = format!("RSI {} indicates Overbought", rsi);
            warn!(
                "NewsHandler: REJECTED Bullish News for {}. {}",
                signal.symbol, reason
            );
            return NewsAction::Rejected(reason);
        }

        // 3. Construct Proposal
        let reason = format!("News (Trend Correct & RSI OK): {}", signal.headline);
        if let Some(mut proposal) = self
            .signal_processor
            .build_proposal(
                config,
                execution_service,
                signal.symbol.clone(),
                OrderSide::Buy,
                price,
                timestamp * 1000,
                reason,
            )
            .await
        {
            proposal.order_type = crate::domain::trading::types::OrderType::Market;
            info!(
                "NewsHandler: Proposing BUY based on Validated News: {}",
                signal.headline
            );
            return NewsAction::Buy(proposal);
        }

        NewsAction::NoAction
    }
}

/// Processes a bearish news signal for an existing position.
///
/// Two scenarios:
/// 1. **Profitable position (>5%)**: Tighten trailing stop to lock gains
/// 2. **Losing/flat position**: Trigger panic sell to limit losses
///
/// # Arguments
/// * `signal` - The news signal to process
/// * `context` - Symbol context with position and indicators
/// * `portfolio_position` - Current position data (quantity, average price)
/// * `current_price` - Current market price
/// * `timestamp` - Current timestamp
///
/// # Returns
/// A `NewsAction` indicating what action was taken.
pub fn process_bearish_news(
    signal: &NewsSignal,
    context: &mut SymbolContext,
    portfolio_position: (Decimal, Decimal), // (quantity, average_price)
    current_price: Decimal,
    timestamp: i64,
) -> NewsAction {
    let (quantity, avg_price) = portfolio_position;
    let pnl_pct = if !avg_price.is_zero() {
        (current_price - avg_price) / avg_price
    } else {
        Decimal::ZERO
    };

    info!(
        "NewsHandler: Processing BEARISH news for {}. PnL: {}%",
        signal.symbol,
        pnl_pct * dec!(100.0)
    );

    if pnl_pct > dec!(0.05) {
        // SCENARIO 1: Profitable Position -> Tighten Stop to Protect Gains
        tighten_stop_on_bearish_news(context, &signal.symbol, current_price);
        return NewsAction::TightenStop;
    }

    // SCENARIO 2: Losing or Flat Position -> Panic Sell
    info!(
        "NewsHandler: News Triggering PANIC SELL for {} to limit potential loss.",
        signal.symbol
    );

    let proposal = TradeProposal {
        symbol: signal.symbol.clone(),
        side: OrderSide::Sell,
        price: Decimal::ZERO,
        quantity, // Sell ALL
        order_type: crate::domain::trading::types::OrderType::Market,
        reason: format!(
            "News Panic Sell (PnL: {}%): {}",
            pnl_pct * dec!(100.0),
            signal.headline
        ),
        timestamp,
    };

    NewsAction::PanicSell(proposal)
}

/// Tightens trailing stop on bearish news for profitable positions.
fn tighten_stop_on_bearish_news(context: &mut SymbolContext, symbol: &str, current_price: Decimal) {
    let atr = context
        .last_features
        .atr
        .unwrap_or(current_price * dec!(0.01));

    // 0.5% gap approximately
    let tight_multiplier =
        (current_price * dec!(0.005)) / if atr.is_zero() { Decimal::ONE } else { atr };

    use crate::application::risk_management::trailing_stops::StopState;

    if let StopState::ActiveStop { stop_price, .. } = &mut context.position_manager.trailing_stop {
        let new_stop = current_price - (atr * tight_multiplier.max(dec!(0.5)));

        if new_stop > *stop_price {
            *stop_price = new_stop;
            info!(
                "NewsHandler: News TIGHTENED Trailing Stop for {} to {} (Locking Gains)",
                symbol, new_stop
            );
        }
    } else {
        // Create new tight stop
        context.position_manager.trailing_stop =
            StopState::on_buy(current_price, atr, tight_multiplier.max(dec!(0.5)));
        info!(
            "NewsHandler: News CREATED Tight Trailing Stop for {}",
            symbol
        );
    }
}

/// Sends a news-generated proposal to the proposal channel.
pub async fn send_news_proposal(
    proposal_tx: &Sender<TradeProposal>,
    proposal: TradeProposal,
) -> Result<(), String> {
    proposal_tx
        .send(proposal)
        .await
        .map_err(|e| format!("Failed to send news proposal: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agents::analyst_config::AnalystConfig;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::DualSMAStrategy;
    use crate::domain::listener::NewsSentiment;
    use crate::domain::trading::symbol_context::SymbolContext;

    fn create_test_context() -> SymbolContext {
        let config = AnalystConfig::default();
        let strategy = Arc::new(DualSMAStrategy::new(20, 60, dec!(0.0)));
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        SymbolContext::new(config, strategy, win_rate_provider, vec![])
    }

    #[test]
    fn test_bearish_news_action_profitable() {
        let mut context = create_test_context();
        context.last_features.atr = Some(dec!(1.0));

        // Setup an active trailing stop
        context.position_manager.trailing_stop =
            crate::application::risk_management::trailing_stops::StopState::on_buy(
                Decimal::from(95),
                Decimal::ONE,
                Decimal::from(3),
            );

        let signal = NewsSignal {
            symbol: "TEST".to_string(),
            headline: "Test bearish news".to_string(),
            sentiment: NewsSentiment::Bearish,
            source: "test".to_string(),
            url: None,
        };

        // Position is 10% profitable (current 110, avg 100)
        let action = process_bearish_news(
            &signal,
            &mut context,
            (Decimal::from(10), Decimal::from(100)),
            Decimal::from(110),
            1000,
        );

        match action {
            NewsAction::TightenStop => (), // Expected
            _ => panic!("Expected TightenStop action for profitable position"),
        }
    }

    #[test]
    fn test_bearish_news_action_losing() {
        let mut context = create_test_context();
        context.last_features.atr = Some(dec!(1.0));

        let signal = NewsSignal {
            symbol: "TEST".to_string(),
            headline: "Test bearish news".to_string(),
            sentiment: NewsSentiment::Bearish,
            source: "test".to_string(),
            url: None,
        };

        // Position is losing (current 95, avg 100)
        let action = process_bearish_news(
            &signal,
            &mut context,
            (Decimal::from(10), Decimal::from(100)),
            Decimal::from(95),
            1000,
        );

        match action {
            NewsAction::PanicSell(proposal) => {
                assert_eq!(proposal.symbol, "TEST");
                assert_eq!(proposal.side, OrderSide::Sell);
                assert_eq!(proposal.quantity, Decimal::from(10));
            }
            _ => panic!("Expected PanicSell action for losing position"),
        }
    }
}
