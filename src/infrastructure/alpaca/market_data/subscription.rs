use super::AlpacaMarketDataService;
use crate::domain::trading::types::MarketEvent;
use anyhow::Result;
use tokio::sync::{
    broadcast,
    mpsc::{self, Receiver},
};

impl AlpacaMarketDataService {
    pub(crate) async fn subscribe_internal(
        &self,
        symbols: Vec<String>,
    ) -> Result<Receiver<MarketEvent>> {
        self.ws_manager.update_subscription(symbols.clone()).await?;
        let mut broadcast_rx = self.ws_manager.subscribe();
        let (tx, rx) = mpsc::channel(100);

        for symbol in symbols {
            let _ = tx.send(MarketEvent::SymbolSubscription { symbol }).await;
        }

        let tx_forward = tx;

        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event) => {
                        if tx_forward.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            "Market event broadcast receiver lagged, missed {} messages",
                            n
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("Market event broadcast channel closed");
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}
