use crate::domain::ports::MarketDataService;
use crate::domain::trading::types::MarketEvent;
use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::info;

#[derive(Clone)]
pub struct MockMarketDataService {
    subscribers: Arc<RwLock<Vec<Sender<MarketEvent>>>>,
    pub simulation_enabled: bool,
    current_prices: Arc<RwLock<std::collections::HashMap<String, Decimal>>>,
}

impl MockMarketDataService {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Vec::new())),
            simulation_enabled: true,
            current_prices: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub fn new_no_sim() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Vec::new())),
            simulation_enabled: false,
            current_prices: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }
}

impl Default for MockMarketDataService {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMarketDataService {
    pub async fn publish(&self, event: MarketEvent) {
        if let MarketEvent::Quote { symbol, price, .. } = &event {
            self.current_prices
                .write()
                .await
                .insert(symbol.clone(), *price);
        }

        let mut subs = self.subscribers.write().await;

        if subs.is_empty() {
            return;
        }

        let mut active_subs = Vec::new();
        let mut sent_count = 0;
        for tx in subs.iter() {
            if tx.send(event.clone()).await.is_ok() {
                active_subs.push(tx.clone());
                sent_count += 1;
            }
        }
        *subs = active_subs;

        if matches!(event, MarketEvent::Quote { symbol, .. } if symbol.contains("BTC")) {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let count = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
            #[allow(clippy::manual_is_multiple_of)]
            if count.is_multiple_of(10) {
                info!(
                    "MockMarketDataService: Published {} events to {} subscribers",
                    count, sent_count
                );
            }
        }
    }

    pub async fn set_price(&self, symbol: &str, price: Decimal) {
        self.current_prices
            .write()
            .await
            .insert(symbol.to_string(), price);

        self.publish(MarketEvent::Quote {
            symbol: symbol.to_string(),
            price,
            quantity: Decimal::ONE,
            timestamp: chrono::Utc::now().timestamp(),
        })
        .await;
    }
}

#[async_trait]
impl MarketDataService for MockMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        let (tx, rx) = mpsc::channel(100);

        self.subscribers.write().await.push(tx.clone());

        let symbols_clone = symbols.clone();
        let service_clone = self.clone();

        if self.simulation_enabled {
            tokio::spawn(async move {
                use chrono::Utc;
                use std::time::Duration;
                use tokio::time;

                let mut prices: std::collections::HashMap<String, f64> =
                    std::collections::HashMap::new();
                let mut iteration = 0u64;

                for symbol in &symbols_clone {
                    let base_price = if symbol.contains("BTC") {
                        96000.0
                    } else if symbol.contains("ETH") {
                        3400.0
                    } else if symbol.contains("AVAX") {
                        40.0
                    } else {
                        150.0
                    };
                    prices.insert(symbol.clone(), base_price);
                }

                info!(
                    "MockMarketDataService: Starting price simulation for {:?}",
                    symbols_clone
                );

                let mut interval = time::interval(Duration::from_millis(500));

                loop {
                    interval.tick().await;
                    iteration += 1;

                    for (idx, symbol) in symbols_clone.iter().enumerate() {
                        let current_price = prices.get(symbol).copied().unwrap_or(100.0);

                        let seed = (iteration + idx as u64) * 1103515245 + 12345;
                        let random_val = (((seed / 65536) % 1000) as f64 / 1000.0) - 0.5;
                        let change_pct = random_val * 0.01;
                        let new_price = current_price * (1.0 + change_pct);

                        prices.insert(symbol.clone(), new_price);

                        let event = MarketEvent::Quote {
                            symbol: symbol.clone(),
                            price: Decimal::from_f64_retain(new_price).unwrap_or(Decimal::ZERO),
                            quantity: Decimal::ONE,
                            timestamp: Utc::now().timestamp(),
                        };

                        service_clone.publish(event).await;
                    }
                }
            });

            info!(
                "MockMarketDataService: Subscribed to {:?} (Simulation Enabled)",
                symbols
            );
        } else {
            info!(
                "MockMarketDataService: Subscribed to {:?} (Simulation Disabled)",
                symbols
            );
        }

        Ok(rx)
    }

    async fn get_tradable_assets(&self) -> Result<Vec<String>> {
        Ok(vec![
            "AAPL".to_string(),
            "MSFT".to_string(),
            "NVDA".to_string(),
            "TSLA".to_string(),
            "GOOGL".to_string(),
            "BTC/USD".to_string(),
            "ETH/USD".to_string(),
        ])
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        Ok(vec![
            "AAPL".to_string(),
            "MSFT".to_string(),
            "NVDA".to_string(),
            "TSLA".to_string(),
            "GOOGL".to_string(),
        ])
    }

    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, rust_decimal::Decimal>> {
        let stored_prices = self.current_prices.read().await;
        let mut result = std::collections::HashMap::new();

        for sym in symbols {
            let price = stored_prices
                .get(&sym)
                .copied()
                .unwrap_or(Decimal::from(100));
            result.insert(sym, price);
        }
        Ok(result)
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
