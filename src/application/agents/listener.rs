use anyhow::{Context, Result};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::domain::listener::{ListenerConfig, ListenerRule, NewsEvent, ListenerAction};
use crate::domain::ports::NewsDataService;
use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};

pub struct ListenerAgent {
    news_service: Arc<dyn NewsDataService>,
    config: ListenerConfig,
    proposal_tx: mpsc::Sender<TradeProposal>,
}

impl ListenerAgent {
    pub fn new(
        news_service: Arc<dyn NewsDataService>,
        config: ListenerConfig,
        proposal_tx: mpsc::Sender<TradeProposal>,
    ) -> Self {
        Self {
            news_service,
            config,
            proposal_tx,
        }
    }

    pub async fn run(&self) {
        info!("Listener Agent started.");

        let mut news_rx = match self.news_service.subscribe_news().await {
            Ok(rx) => rx,
            Err(e) => {
                error!("Failed to subscribe to news service: {}. Listener Agent stopping.", e);
                return;
            }
        };

        while let Some(event) = news_rx.recv().await {
            info!("Received news event: {} - {}", event.source, event.title);
            self.process_event(&event).await;
        }

        warn!("Listener Agent stopped (stream ended).");
    }

    async fn process_event(&self, event: &NewsEvent) {
        for rule in &self.config.rules {
            if !rule.active {
                continue;
            }

            if rule.matches(&event.content) || rule.matches(&event.title) {
                info!("News matched rule '{}': {}", rule.id, event.title);
                if let Err(e) = self.trigger_action(rule, event).await {
                    error!("Failed to trigger action for rule '{}': {}", rule.id, e);
                }
            }
        }
    }

    async fn trigger_action(&self, rule: &ListenerRule, event: &NewsEvent) -> Result<()> {
        let side = match rule.action {
            ListenerAction::BuyImmediate => OrderSide::Buy,
            ListenerAction::SellImmediate => OrderSide::Sell,
        };

        // For now, we propose a Market order.
        // Quantity and exact price handling would ideally be dynamic or come from a smarter sizing logic.
        // Here we put a placeholder quantity, expecting RiskManager to validate/sizing or reject.
        // BUT: RiskManager expects a quantity. 
        // Let's use a "unit" quantity or a minimal viable amount, 
        // OR we can make the Proposal accept a "Notional" amount if we supported it.
        // Since we don't know the price yet (unless we fetch it), we might need to send a proposal 
        // that indicates "Buy as much as possible" or "Buy fixed amount".
        // 
        // Strategy: 
        // 1. We don't have price here. Market Scanner has prices.
        // 2. We can send a Market order with a "0" price (standard for market orders usually) 
        //    but Quantity is tricky without price.
        //
        // SIMPLIFICATION:
        // We will infer a "standard" trade size from config if we had access to it, 
        // or just put 1.0 and assume RiskManager/Executor might fix it or we accept 1 unit for now.
        // Better: ListenerAgent should probably have access to current prices or just make a best effort.
        
        let proposal = TradeProposal {
            symbol: rule.target_symbol.clone(),
            side,
            price: Decimal::ZERO, // Market order
            quantity: Decimal::new(100, 0), // Placeholder: 100 units (e.g. 100 Doge). 
                                            // TODO: Make this configurable in ListenerRule or calculate based on % equity.
            order_type: OrderType::Market,
            reason: format!("Listener Trigger: {} (News: {})", rule.id, event.title),
            timestamp: chrono::Utc::now().timestamp(),
        };

        info!("Generating Trade Proposal based on news: {:?}", proposal);

        self.proposal_tx.send(proposal).await.context("Failed to send trade proposal")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    // Mock News Service for Unit Test (Simpler than the infra one)
    struct TestNewsService {
        rx: tokio::sync::Mutex<Option<mpsc::Receiver<NewsEvent>>>,
    }

    #[async_trait::async_trait]
    impl NewsDataService for TestNewsService {
        async fn subscribe_news(&self) -> Result<mpsc::Receiver<NewsEvent>> {
            Ok(self.rx.lock().await.take().unwrap())
        }
    }

    #[tokio::test]
    async fn test_listener_agent_matches_and_proposes() {
        let (news_tx, news_rx) = mpsc::channel(10);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(10);

        let news_service = Arc::new(TestNewsService {
            rx: tokio::sync::Mutex::new(Some(news_rx)),
        });

        let config = ListenerConfig {
            poll_interval_seconds: 1,
            rules: vec![ListenerRule {
                id: "test-rule".to_string(),
                keywords: vec!["Buy".to_string(), "Now".to_string()],
                target_symbol: "TEST/USD".to_string(),
                action: ListenerAction::BuyImmediate,
                active: true,
            }],
        };

        let agent = ListenerAgent::new(news_service, config, proposal_tx);

        // Spawn agent
        tokio::spawn(async move {
            agent.run().await;
        });

        // Send matching news
        let event = NewsEvent {
            id: Uuid::new_v4().to_string(),
            source: "Test".to_string(),
            title: "Signal".to_string(),
            content: "You should Buy Now!".to_string(),
            url: None,
            timestamp: Utc::now(),
            sentiment_score: None,
        };
        news_tx.send(event).await.unwrap();

        // Check proposal
        let proposal = proposal_rx.recv().await;
        assert!(proposal.is_some());
        let p = proposal.unwrap();
        assert_eq!(p.symbol, "TEST/USD");
        assert_eq!(p.side, OrderSide::Buy);
    }
}
