use crate::application::agents::sentinel::SentinelCommand;
use crate::domain::ports::{ExecutionService, MarketDataService};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::time::{self, Duration};
use tracing::{error, info, warn};

pub struct MarketScanner {
    market_service: Arc<dyn MarketDataService>,
    execution_service: Arc<dyn ExecutionService>,
    sentinel_cmd_tx: Sender<SentinelCommand>,
    scan_interval: Duration,
    is_enabled: bool,
    agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
}

impl MarketScanner {
    pub fn new(
        market_service: Arc<dyn MarketDataService>,
        execution_service: Arc<dyn ExecutionService>,
        sentinel_cmd_tx: Sender<SentinelCommand>,
        scan_interval: Duration,
        is_enabled: bool,
        agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    ) -> Self {
        Self {
            market_service,
            execution_service,
            sentinel_cmd_tx,
            scan_interval,
            is_enabled,
            agent_registry,
        }
    }

    pub async fn run(&self) {
        if !self.is_enabled {
            info!("MarketScanner is disabled.");
            return;
        }

        info!(
            "MarketScanner started. Scan Interval: {:?}",
            self.scan_interval
        );

        let mut scan_interval = time::interval(self.scan_interval);
        // The first tick completes immediately
        scan_interval.tick().await;

        let mut heartbeat_interval = time::interval(Duration::from_secs(5));

        // Initial Heartbeat
        self.agent_registry
            .update_heartbeat(
                "Scanner",
                crate::application::monitoring::agent_status::HealthStatus::Healthy,
            )
            .await;

        loop {
            tokio::select! {
                _ = heartbeat_interval.tick() => {
                    self.agent_registry
                        .update_heartbeat(
                            "Scanner",
                            crate::application::monitoring::agent_status::HealthStatus::Healthy,
                        )
                        .await;
                }

                _ = scan_interval.tick() => {
                    // 1. Get Top Movers
                    let mut symbols = match self.market_service.get_top_movers().await {
                        Ok(s) => {
                            info!("MarketScanner: Top movers found: {:?}", s);
                             self.agent_registry
                                .update_metric("Scanner", "top_movers_count", s.len().to_string())
                                .await;
                            s
                        }
                        Err(e) => {
                            error!("MarketScanner: Failed to fetch top movers: {}", e);
                            vec![]
                        }
                    };

                    // 2. Get Portfolio Holdings
                    match self.execution_service.get_portfolio().await {
                        Ok(portfolio) => {
                            let held_symbols: Vec<String> = portfolio.positions.keys().cloned().collect();
                            if !held_symbols.is_empty() {
                                info!("MarketScanner: Including held symbols: {:?}", held_symbols);
                                for sym in held_symbols {
                                    if !symbols.contains(&sym) {
                                        symbols.push(sym);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!(
                                "MarketScanner: Failed to fetch portfolio to preserve held assets: {}",
                                e
                            );
                        }
                    }

                    // 3. Send Update
                    if !symbols.is_empty() {
                        info!(
                            "MarketScanner: Sending {} symbols to Sentinel: {:?}",
                            symbols.len(),
                            symbols
                        );
                        if let Err(e) = self
                            .sentinel_cmd_tx
                            .send(SentinelCommand::UpdateSymbols(symbols))
                            .await
                        {
                            error!("MarketScanner: Failed to update Sentinel: {}", e);
                            break;
                        }
                    } else {
                        warn!("MarketScanner: No top movers found, nothing to send to Sentinel");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::{ExecutionService, MarketDataService, OrderUpdate};
    use crate::domain::trading::portfolio::{Portfolio, Position};

    use crate::domain::trading::types::{MarketEvent, Order};
    use anyhow::Result;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use std::sync::Mutex;
    use tokio::sync::RwLock;
    use tokio::sync::mpsc;

    struct MockScannerService {
        movers: Mutex<Option<Vec<String>>>,
    }

    #[async_trait]
    impl MarketDataService for MockScannerService {
        async fn subscribe(&self, _symbols: Vec<String>) -> Result<mpsc::Receiver<MarketEvent>> {
            unimplemented!()
        }

        async fn get_tradable_assets(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }

        async fn get_top_movers(&self) -> Result<Vec<String>> {
            let mut guard = self.movers.lock().unwrap();
            let movers = guard.take().unwrap_or_default();
            Ok(movers)
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

    struct MockExecService {
        portfolio: Arc<RwLock<Portfolio>>,
    }

    #[async_trait]
    impl ExecutionService for MockExecService {
        async fn execute(&self, _order: Order) -> Result<()> {
            unimplemented!()
        }
        async fn get_portfolio(&self) -> Result<Portfolio> {
            Ok(self.portfolio.read().await.clone())
        }
        async fn get_today_orders(&self) -> Result<Vec<Order>> {
            unimplemented!()
        }
        async fn get_open_orders(&self) -> Result<Vec<Order>> {
            unimplemented!()
        }
        async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<()> {
            Ok(())
        }
        async fn cancel_all_orders(&self) -> Result<()> {
            Ok(())
        }
        async fn subscribe_order_updates(
            &self,
        ) -> Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
            let (_tx, rx) = tokio::sync::broadcast::channel(1);

            Ok(rx)
        }
    }

    #[tokio::test]
    async fn test_scanner_sends_update() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(10);

        let service = Arc::new(MockScannerService {
            movers: Mutex::new(Some(vec!["AAPL".to_string(), "GOOG".to_string()])),
        });

        // Held positions
        let mut port = Portfolio::new();
        port.positions.insert(
            "MSFT".to_string(),
            Position {
                symbol: "MSFT".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::ZERO,
            },
        );
        // AAPL is also held, to test dedup
        port.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: Decimal::from(5),
                average_price: Decimal::ZERO,
            },
        );

        let exec_service = Arc::new(MockExecService {
            portfolio: Arc::new(RwLock::new(port)),
        });

        let scanner = MarketScanner::new(
            service,
            exec_service,
            cmd_tx,
            Duration::from_millis(100),
            true,
            Arc::new(
                crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                    crate::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
        );

        tokio::spawn(async move {
            scanner.run().await;
        });

        // Should receive the update
        let update = cmd_rx.recv().await.expect("Should receive update");

        // Check for AAPL, GOOG (movers) and MSFT (held)
        if let crate::application::agents::sentinel::SentinelCommand::UpdateSymbols(symbols) =
            update
        {
            assert!(symbols.contains(&"AAPL".to_string()));
            assert!(symbols.contains(&"GOOG".to_string()));
            assert!(symbols.contains(&"MSFT".to_string()));
            // Logic might change order, but all 3 should be there.
            // Size should be 3 because AAPL is deduped.
            assert_eq!(symbols.len(), 3);
        } else {
            panic!("Expected UpdateSymbols, got {:?}", update);
        }
    }
}
