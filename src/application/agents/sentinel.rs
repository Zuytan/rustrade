use crate::application::monitoring::connection_health_service::{
    ConnectionHealthService, ConnectionStatus,
};
use crate::application::monitoring::heartbeat::StreamHealthMonitor;
use crate::domain::ports::MarketDataService;
use crate::domain::trading::types::MarketEvent;
use crate::domain::validation::data_quality::StrictEventValidator;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{error, info, warn};

#[derive(Debug)]
pub enum SentinelCommand {
    Shutdown,
    UpdateSymbols(Vec<String>),
    /// Request available tradable symbols from the market data service
    LoadAvailableSymbols(tokio::sync::oneshot::Sender<Vec<String>>),
    /// Request top movers (by volume)
    LoadTopMovers(tokio::sync::oneshot::Sender<Vec<String>>),
}

pub struct Sentinel {
    market_service: Arc<dyn MarketDataService>,
    market_tx: Sender<MarketEvent>,
    symbols: Vec<String>,
    cmd_rx: Option<Receiver<SentinelCommand>>,
    health_service: Arc<ConnectionHealthService>,
    heartbeat: StreamHealthMonitor,
    last_heal_attempt: Option<std::time::Instant>,
}

impl Sentinel {
    pub fn new(
        market_service: Arc<dyn MarketDataService>,
        market_tx: Sender<MarketEvent>,
        symbols: Vec<String>,
        cmd_rx: Option<Receiver<SentinelCommand>>,
        health_service: Arc<ConnectionHealthService>,
    ) -> Self {
        // Crypto threshold: 10s for silence
        let heartbeat = StreamHealthMonitor::new("Sentinel", Duration::from_secs(10));

        Self {
            market_service,
            market_tx,
            symbols,
            cmd_rx,
            health_service,
            heartbeat,
            last_heal_attempt: None,
        }
    }

    pub async fn run(&mut self) {
        let mut current_symbols = self.symbols.clone();

        info!("Sentinel subscribing to: {:?}", current_symbols);
        self.health_service
            .set_market_data_status(ConnectionStatus::Offline, Some("Initializing".to_string()))
            .await;

        // Single subscription to the shared WebSocket
        let mut market_rx = match self.market_service.subscribe(current_symbols.clone()).await {
            Ok(rx) => {
                self.health_service
                    .set_market_data_status(ConnectionStatus::Online, None)
                    .await;
                rx
            }
            Err(e) => {
                error!("Sentinel subscribe failed: {}", e);
                self.health_service
                    .set_market_data_status(
                        ConnectionStatus::Offline,
                        Some(format!("Initial subscribe failed: {}", e)),
                    )
                    .await;
                return;
            }
        };

        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(2));

        loop {
            tokio::select! {
                maybe_event = market_rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            // 1. Update Heartbeat
                            self.heartbeat.record_event();

                            // 2. Validate Event
                            if !StrictEventValidator::validate_event(&event) {
                                // Event dropped by validator, skip forwarding
                                continue;
                            }

                            // 3. Forward Event
                            if let Err(e) = self.market_tx.send(event).await {
                                error!("Sentinel: Failed to forward event: {}", e);
                                return; // Fatal: internal channel closed
                            }
                        }
                        None => {
                            warn!("Sentinel market stream ended. Market data processing stopped.");
                            self.health_service.set_market_data_status(ConnectionStatus::Offline, Some("Market stream ended".to_string())).await;
                            return;
                        }
                    }
                }

                // 4. Periodic Heartbeat Check
                _ = heartbeat_interval.tick() => {
                    if !self.heartbeat.is_healthy() {
                        let elapsed = self.heartbeat.last_event_elapsed();
                        let msg = format!("No data received for {:?}", elapsed);
                        self.health_service.set_market_data_status(
                            ConnectionStatus::Offline,
                            Some(msg)
                        ).await;

                        // Phase 2: Self-healing
                        // If it's very silent (e.g. > 30s), try to re-subscribe
                        // Throttled to once every 60s to avoid hammering
                        let cooldown = Duration::from_secs(60);
                        let can_heal = self.last_heal_attempt.map(|t| t.elapsed() > cooldown).unwrap_or(true);

                        if elapsed > Duration::from_secs(30) && can_heal {
                            warn!("Sentinel: Stream silent for >30s ({:?}). Attempting forced re-subscription...", elapsed);
                            self.last_heal_attempt = Some(std::time::Instant::now());

                            match self.market_service.subscribe(current_symbols.clone()).await {
                                Ok(new_rx) => {
                                    market_rx = new_rx;
                                    self.heartbeat.record_event(); // Reset heartbeat to avoid immediate retry
                                    info!("Sentinel: Forced re-subscription successful for symbols: {:?}", current_symbols);
                                }
                                Err(e) => {
                                    error!("Sentinel: Forced re-subscription failed: {}", e);
                                }
                            }
                        }
                    } else if self.health_service.get_market_data_status().await == ConnectionStatus::Offline {
                         // Auto-recover status if heartbeat is back
                         self.health_service.set_market_data_status(ConnectionStatus::Online, None).await;
                    }
                }

                // Only poll cmd_rx if it exists
                maybe_cmd = async {
                    if let Some(rx) = &mut self.cmd_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    match maybe_cmd {
                        Some(cmd) => {
                            match cmd {
                                SentinelCommand::Shutdown => {
                                    warn!("Sentinel received Shutdown command. Exiting loop.");
                                    return;
                                }
                                SentinelCommand::UpdateSymbols(new_symbols) => {
                                    // Skip if symbols haven't changed
                                    if new_symbols == current_symbols {
                                        info!("Sentinel: Symbols unchanged, skipping update");
                                        continue;
                                    }

                                    info!("Sentinel: Updating subscription to {:?}", new_symbols);

                                    // Update subscription WITHOUT creating new connection
                                    // The WebSocket manager handles this dynamically
                                    match self.market_service.subscribe(new_symbols.clone()).await {
                                        Ok(new_rx) => {
                                            market_rx = new_rx;
                                            current_symbols = new_symbols;
                                            info!("Sentinel: Subscription updated and receiver replaced");
                                        }
                                        Err(e) => {
                                            error!("Sentinel: Failed to update subscription: {}", e);
                                        }
                                    }
                                }
                                SentinelCommand::LoadAvailableSymbols(response_tx) => {
                                    info!("Sentinel: Loading available symbols from market data service");
                                    match self.market_service.get_tradable_assets().await {
                                        Ok(symbols) => {
                                            info!("Sentinel: Loaded {} available symbols", symbols.len());
                                            if response_tx.send(symbols).is_err() {
                                                error!("Sentinel: Failed to send symbols - receiver dropped");
                                            }
                                        }
                                        Err(e) => {
                                            error!("Sentinel: Failed to load symbols: {}", e);
                                            let _ = response_tx.send(Vec::new());
                                        }
                                    }
                                }
                                SentinelCommand::LoadTopMovers(response_tx) => {
                                    info!("Sentinel: Loading Top Movers from market data service");
                                    match self.market_service.get_top_movers().await {
                                        Ok(symbols) => {
                                            info!("Sentinel: Loaded {} top movers", symbols.len());
                                            if response_tx.send(symbols).is_err() {
                                                error!("Sentinel: Failed to send top movers - receiver dropped");
                                            }
                                        }
                                        Err(e) => {
                                            error!("Sentinel: Failed to load top movers: {}", e);
                                            let _ = response_tx.send(Vec::new());
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            info!("Sentinel command channel closed.");
                            self.cmd_rx = None;
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
    use crate::domain::trading::types::MarketEvent;
    use anyhow::Result;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec; // Added import
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

        async fn get_tradable_assets(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }

        async fn get_top_movers(&self) -> Result<Vec<String>> {
            Ok(vec!["ETH/USD".to_string()])
        }

        async fn get_prices(
            &self,
            _symbols: Vec<String>,
        ) -> Result<std::collections::HashMap<String, Decimal>> {
            Ok(std::collections::HashMap::new())
        }

        async fn get_historical_bars(
            &self,
            _symbol: &str,
            _start: chrono::DateTime<chrono::Utc>,
            _end: chrono::DateTime<chrono::Utc>,
            _timeframe: &str,
        ) -> Result<Vec<crate::domain::trading::types::Candle>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_sentinel_forwards_events() {
        let (market_tx, mut market_rx) = mpsc::channel(10);

        let expected_event = MarketEvent::Quote {
            symbol: "ETH/USD".to_string(),
            price: Decimal::from(3000),
            quantity: dec!(1.0),
            timestamp: 1234567890,
        };

        let service = Arc::new(TestMarketDataService {
            events: vec![expected_event.clone()],
        });

        let mut sentinel = Sentinel::new(
            service,
            market_tx,
            vec!["ETH/USD".to_string()],
            None,
            Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new()),
        );

        tokio::spawn(async move {
            sentinel.run().await;
        });

        let received = market_rx.recv().await.expect("Should receive event");

        match received {
            MarketEvent::Quote {
                symbol,
                price,
                timestamp,
                ..
            } => {
                assert_eq!(symbol, "ETH/USD");
                assert_eq!(price, Decimal::from(3000));
                assert_eq!(timestamp, 1234567890);
            }
            MarketEvent::Candle(_) => panic!("Unexpected candle event"),
            MarketEvent::SymbolSubscription { .. } => panic!("Unexpected subscription event"),
        }
    }
}
