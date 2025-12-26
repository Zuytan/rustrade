use crate::domain::ports::MarketDataService;
use crate::domain::types::MarketEvent;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{self, Duration};
use tracing::{error, info, warn};
use std::sync::Arc;

pub struct Sentinel {
    market_service: Arc<dyn MarketDataService>,
    market_tx: Sender<MarketEvent>,
    symbols: Vec<String>,
    update_rx: Option<Receiver<Vec<String>>>,
}

impl Sentinel {
    pub fn new(
        market_service: Arc<dyn MarketDataService>,
        market_tx: Sender<MarketEvent>,
        symbols: Vec<String>,
        update_rx: Option<Receiver<Vec<String>>>,
    ) -> Self {
        Self {
            market_service,
            market_tx,
            symbols,
            update_rx,
        }
    }

    pub async fn run(&mut self) {
        let mut current_symbols = self.symbols.clone();

        loop {
            info!("Sentinel subscribing to: {:?}", current_symbols);
            
            // Allow override of subscription logic if empty symbols? 
            // Mock might handle empty, Alpaca might error.
            if current_symbols.is_empty() {
                warn!("Sentinel has no symbols to subscribe to. Waiting for update...");
            }

            let mut market_rx = match self.market_service.subscribe(current_symbols.clone()).await {
                Ok(rx) => rx,
                Err(e) => {
                    error!("Sentinel subscribe failed: {}. Retrying in 5s...", e);
                    time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            loop {
                tokio::select! {
                    maybe_event = market_rx.recv() => {
                        match maybe_event {
                            Some(event) => {
                                if let Err(e) = self.market_tx.send(event).await {
                                    error!("Sentinel: Failed to forward event: {}", e);
                                    return; // Fatal: internal channel closed
                                }
                            }
                            None => {
                                warn!("Sentinel market stream ended. Re-subscribing...");
                                break;
                            }
                        }
                    }

                    // Only poll update_rx if it exists
                    maybe_update = async {
                        if let Some(rx) = &mut self.update_rx {
                            rx.recv().await
                        } else {
                            std::future::pending().await
                        }
                    } => {
                        match maybe_update {
                            Some(new_symbols) => {
                                info!("Sentinel received new symbols: {:?}", new_symbols);
                                current_symbols = new_symbols;
                                break; // Break inner loop to re-subscribe
                            }
                            None => {
                                info!("Sentinel update channel closed.");
                                self.update_rx = None;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::MarketDataService;
    use crate::domain::types::MarketEvent;
    use anyhow::Result;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tokio::sync::mpsc;

    struct TestMarketDataService {
        events: Vec<MarketEvent>,
    }

    #[async_trait]
    impl MarketDataService for TestMarketDataService {
        async fn subscribe(&self, _symbols: Vec<String>) -> Result<mpsc::Receiver<MarketEvent>> {
            let (tx, rx) = mpsc::channel(10);
            for event in &self.events {
                tx.send(event.clone()).await.unwrap();
            }
            Ok(rx)
        }

        async fn get_top_movers(&self) -> Result<Vec<String>> {
            Ok(vec!["ETH/USD".to_string()])
        }
    }

    #[tokio::test]
    async fn test_sentinel_forwards_events() {
        let (market_tx, mut market_rx) = mpsc::channel(10);

        let expected_event = MarketEvent::Quote {
            symbol: "ETH/USD".to_string(),
            price: Decimal::from(3000),
            timestamp: 1234567890,
        };

        let service = Arc::new(TestMarketDataService {
            events: vec![expected_event.clone()],
        });

        let mut sentinel = Sentinel::new(service, market_tx, vec!["ETH/USD".to_string()], None);

        tokio::spawn(async move {
            sentinel.run().await;
        });

        let received = market_rx.recv().await.expect("Should receive event");

        match received {
            MarketEvent::Quote {
                symbol,
                price,
                timestamp,
            } => {
                assert_eq!(symbol, "ETH/USD");
                assert_eq!(price, Decimal::from(3000));
                assert_eq!(timestamp, 1234567890);
            }
        }
    }
}
