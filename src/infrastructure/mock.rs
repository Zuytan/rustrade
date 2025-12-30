use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::trading::types::{MarketEvent, Order};
use anyhow::Result;
use async_trait::async_trait;
// use chrono::Utc;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::sync::Arc;
// use std::time::Duration;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    RwLock,
};
// use tokio::time;
use tracing::info;

#[derive(Clone)]
pub struct MockMarketDataService {
    subscribers: Arc<RwLock<Vec<Sender<MarketEvent>>>>,
    pub simulation_enabled: bool,
}

impl MockMarketDataService {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Vec::new())),
            simulation_enabled: true,
        }
    }

    pub fn new_no_sim() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Vec::new())),
            simulation_enabled: false,
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
        let mut subs = self.subscribers.write().await;

        if subs.is_empty() {
            // info!("MockMarketDataService: No subscribers for event: {:?}", event);
            return;
        }

        // retain only active subscribers
        let mut active_subs = Vec::new();
        let mut sent_count = 0;
        for tx in subs.iter() {
            if tx.send(event.clone()).await.is_ok() {
                active_subs.push(tx.clone());
                sent_count += 1;
            }
        }
        *subs = active_subs;

        // Log every 10th event to avoid spam
        if matches!(event, MarketEvent::Quote { symbol, .. } if symbol.contains("BTC")) {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let count = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
            #[allow(clippy::manual_is_multiple_of)]
            if count % 10 == 0 {
                info!(
                    "MockMarketDataService: Published {} events to {} subscribers",
                    count, sent_count
                );
            }
        }
    }
}

#[async_trait]
impl MarketDataService for MockMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        let (tx, rx) = mpsc::channel(100);

        // Add to subscribers
        self.subscribers.write().await.push(tx.clone());

        let symbols_clone = symbols.clone();
        let service_clone = self.clone();

        if self.simulation_enabled {
            // Spawn random walk simulation for demo/testing
            tokio::spawn(async move {
                use chrono::Utc;
                use std::time::Duration;
                use tokio::time;

                let mut prices: std::collections::HashMap<String, f64> =
                    std::collections::HashMap::new();
                let mut iteration = 0u64;

                // Initialize prices
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

                        // Simple pseudo-random using iteration and timestamp
                        // This creates -0.5% to +0.5% variance
                        let seed = (iteration + idx as u64) * 1103515245 + 12345;
                        let random_val = (((seed / 65536) % 1000) as f64 / 1000.0) - 0.5; // -0.5 to +0.5
                        let change_pct = random_val * 0.01;
                        let new_price = current_price * (1.0 + change_pct);

                        prices.insert(symbol.clone(), new_price);

                        let event = MarketEvent::Quote {
                            symbol: symbol.clone(),
                            price: Decimal::from_f64(new_price).unwrap_or(Decimal::ZERO),
                            timestamp: Utc::now().timestamp_millis(),
                        };

                        service_clone.publish(event).await;
                    }
                }
            });

            info!("MockMarketDataService: Subscribed to {:?} (Simulation Enabled)", symbols);
        } else {
            info!("MockMarketDataService: Subscribed to {:?} (Simulation Disabled)", symbols);
        }

        Ok(rx)
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
        let mut prices = std::collections::HashMap::new();
        // Return some dummy prices or 100.0 for everything
        // For E2E tests, this might be tricky if we want specific values.
        // We could use a shared map in the struct to store "current" prices that can be set by tests?
        // For now, let's just return a constant for simplicity or randomized variations.
        // To trigger the circuit breaker test, we might need a way to inject a "crash" price.
        // CHECK: The MockMarketDataService doesn't accept external price updates in this simple version
        // except via publish(). But get_prices is Pull.
        // We will return $100.0 for everything as a baseline.

        for sym in symbols {
            // If we want to simulate a crash for TSLA in the test, we might hardcode it here?
            // That's ugly for general use.
            // Better: RiskManager test will likely mock the service trait directly OR
            // we can add a `set_price` method to MockMarketDataService.

            // Check if it's TSLA for the specific test case? No, that's bad.
            // Let's implement a rudimentary price store in MockMarketDataService later if needed.
            // For now: $100.0.
            prices.insert(sym, rust_decimal::Decimal::from(100));
        }
        Ok(prices)
    }

    async fn get_historical_bars(
        &self,
        _symbol: &str,
        _start: chrono::DateTime<chrono::Utc>,
        _end: chrono::DateTime<chrono::Utc>,
        _timeframe: &str,
    ) -> Result<Vec<crate::domain::trading::types::Candle>> {
        // For now, return empty or mock data
        Ok(vec![])
    }
}

use crate::domain::trading::portfolio::Portfolio;

pub struct MockExecutionService {
    portfolio: Arc<RwLock<Portfolio>>,
    orders: Arc<RwLock<Vec<Order>>>,
    slippage_pct: f64,         // Slippage as decimal (e.g., 0.001 = 0.1%)
    commission_per_share: f64, // Commission per share (e.g., 0.001 = $0.001/share)
}

impl MockExecutionService {
    pub fn new(portfolio: Arc<RwLock<Portfolio>>) -> Self {
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            slippage_pct: 0.0,         // Default: no slippage for existing tests
            commission_per_share: 0.0, // Default: no commission for existing tests
        }
    }

    /// Create with transaction costs (for realistic backtests)
    pub fn with_costs(
        portfolio: Arc<RwLock<Portfolio>>,
        slippage_pct: f64,
        commission_per_share: f64,
    ) -> Self {
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            slippage_pct,
            commission_per_share,
        }
    }
}

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute(&self, order: Order) -> Result<()> {
        info!("MockExecution: Placing order {}...", order.id);
        // Faster execution for tests
        // time::sleep(Duration::from_millis(200)).await;

        // Simulate execution update on the "exchange" side
        let mut port = self.portfolio.write().await;

        // Apply slippage to execution price
        let slippage_multiplier = Decimal::from_f64(match order.side {
            crate::domain::trading::types::OrderSide::Buy => 1.0 + self.slippage_pct, // Buy higher
            crate::domain::trading::types::OrderSide::Sell => 1.0 - self.slippage_pct, // Sell lower
        })
        .unwrap_or(Decimal::ONE);

        let execution_price = order.price * slippage_multiplier;
        let cost = execution_price * order.quantity;

        // Calculate commission
        let commission =
            Decimal::from_f64(self.commission_per_share).unwrap_or(Decimal::ZERO) * order.quantity;

        if self.slippage_pct > 0.0 || self.commission_per_share > 0.0 {
            info!(
                "MockExecution: Order {} - Slippage: {:.4}%, Commission: ${:.4}, Total Cost Impact: ${:.4}",
                order.id,
                self.slippage_pct * 100.0,
                commission,
                (execution_price - order.price).abs() * order.quantity + commission
            );
        }

        match order.side {
            crate::domain::trading::types::OrderSide::Buy => {
                // Deduct cost + commission from cash
                port.cash -= cost + commission;
                let pos = port.positions.entry(order.symbol.clone()).or_insert(
                    crate::domain::trading::portfolio::Position {
                        symbol: order.symbol.clone(),
                        quantity: Decimal::ZERO,
                        average_price: Decimal::ZERO,
                    },
                );

                let total_qty = pos.quantity + order.quantity;
                let total_cost = (pos.quantity * pos.average_price) + cost;
                if total_qty > Decimal::ZERO {
                    pos.average_price = total_cost / total_qty;
                }
                pos.quantity = total_qty;
            }
            crate::domain::trading::types::OrderSide::Sell => {
                // Add proceeds minus commission to cash
                port.cash += cost - commission;
                let pos = port.positions.entry(order.symbol.clone()).or_insert(
                    crate::domain::trading::portfolio::Position {
                        symbol: order.symbol.clone(),
                        quantity: Decimal::ZERO,
                        average_price: Decimal::ZERO,
                    },
                );
                pos.quantity -= order.quantity;
            }
        }

        // Record order
        self.orders.write().await.push(order.clone());

        info!(
            "MockExecution: Order {} placed and executed on Exchange.",
            order.id
        );
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        let port = self.portfolio.read().await;
        Ok(port.clone())
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        let orders = self.orders.read().await;
        Ok(orders.clone())
    }
}

pub struct NullTradeRepository;

#[async_trait]
impl crate::domain::repositories::TradeRepository for NullTradeRepository {
    async fn save(&self, _trade: &Order) -> Result<()> {
        Ok(())
    }
    async fn find_by_symbol(&self, _symbol: &str) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn find_recent(&self, _limit: usize) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn get_all(&self) -> Result<Vec<Order>> {
        Ok(vec![])
    }
    async fn count(&self) -> Result<usize> {
        Ok(0)
    }
}

pub struct NullCandleRepository;

#[async_trait]
impl crate::domain::repositories::CandleRepository for NullCandleRepository {
    async fn save(&self, _candle: &crate::domain::trading::types::Candle) -> Result<()> {
        Ok(())
    }
    async fn get_range(
        &self,
        _symbol: &str,
        _start_ts: i64,
        _end_ts: i64,
    ) -> Result<Vec<crate::domain::trading::types::Candle>> {
        Ok(vec![])
    }
    async fn prune(&self, _days_retention: i64) -> Result<u64> {
        Ok(0)
    }
}

pub struct NullStrategyRepository;

#[async_trait]
impl crate::domain::repositories::StrategyRepository for NullStrategyRepository {
    async fn save(
        &self,
        _config: &crate::domain::market::strategy_config::StrategyDefinition,
    ) -> Result<()> {
        Ok(())
    }
    async fn find_by_symbol(
        &self,
        _symbol: &str,
    ) -> Result<Option<crate::domain::market::strategy_config::StrategyDefinition>> {
        Ok(None)
    }
    async fn get_all_active(
        &self,
    ) -> Result<Vec<crate::domain::market::strategy_config::StrategyDefinition>> {
        Ok(vec![])
    }
}
