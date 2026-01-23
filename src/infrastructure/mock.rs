use crate::domain::ports::{ExecutionService, MarketDataService, OrderUpdate};
use crate::domain::trading::fee_model::{ConstantFeeModel, FeeModel}; // Added
use crate::domain::trading::types::{MarketEvent, Order};
use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::{
    RwLock,
    mpsc::{self, Receiver, Sender},
};
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
            if count % 10 == 0 {
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
                            price: Decimal::from_f64(new_price).unwrap_or(Decimal::ZERO),
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

use crate::domain::trading::portfolio::Portfolio;

pub struct MockExecutionService {
    portfolio: Arc<RwLock<Portfolio>>,
    orders: Arc<RwLock<Vec<Order>>>,
    fee_model: Arc<dyn FeeModel>,
    order_update_sender: broadcast::Sender<OrderUpdate>,
}

impl MockExecutionService {
    pub fn new(portfolio: Arc<RwLock<Portfolio>>) -> Self {
        {
            // Initializing sync immediately for Mock
            // Use try_write to avoid panicking in async context (blocking_write is forbidden)
            if let Ok(mut guard) = portfolio.try_write() {
                guard.synchronized = true;
            } else {
                 // If we can't get the lock, we assume it might be locked by the test setup
                 // or already synchronized. We log a warning but don't panic.
                 tracing::warn!("MockExecutionService: Could not acquire lock to set synchronized=true. Assuming handled elsewhere.");
            }
        }
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            fee_model: Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO)),
            order_update_sender: broadcast::channel(100).0,
        }
    }

    pub fn with_costs(portfolio: Arc<RwLock<Portfolio>>, fee_model: Arc<dyn FeeModel>) -> Self {
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            fee_model,
            order_update_sender: broadcast::channel(100).0,
        }
    }
}

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute(&self, order: Order) -> Result<()> {
        info!("MockExecution: Placing order {}...", order.id);

        let mut port =
            tokio::time::timeout(std::time::Duration::from_secs(2), self.portfolio.write())
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "MockExecution: Deadlock detected acquiring Portfolio write lock"
                    )
                })?;

        // Calculate costs using FeeModel
        let costs = self
            .fee_model
            .calculate_cost(order.quantity, order.price, order.side);

        let slippage_amount = costs.slippage_cost; // Value lost due to slippage
        let commission = costs.fee;

        // Effective price adjustment due to slippage
        // For Buy: we pay more -> price + slippage_per_unit
        // For Sell: we get less -> price - slippage_per_unit
        let slippage_per_unit = if order.quantity > Decimal::ZERO {
            slippage_amount / order.quantity
        } else {
            Decimal::ZERO
        };

        let execution_price = match order.side {
            crate::domain::trading::types::OrderSide::Buy => order.price + slippage_per_unit,
            crate::domain::trading::types::OrderSide::Sell => order.price - slippage_per_unit,
        };

        // Total cost value (base value)
        let cost = execution_price * order.quantity;

        if !commission.is_zero() || !slippage_amount.is_zero() {
            info!(
                "MockExecution: Order {} - Slippage: ${:.4}, Commission: ${:.4}, Total Impact: ${:.4}",
                order.id, slippage_amount, commission, costs.total_impact
            );
        }

        match order.side {
            crate::domain::trading::types::OrderSide::Buy => {
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

        self.orders.write().await.push(order.clone());

        let _ = self.order_update_sender.send(OrderUpdate {
            order_id: order.id.clone(),
            client_order_id: order.id.clone(),
            symbol: order.symbol.clone(),
            side: order.side,
            status: crate::domain::trading::types::OrderStatus::Filled,
            filled_qty: order.quantity,
            filled_avg_price: Some(execution_price),
            timestamp: chrono::Utc::now(),
        });

        info!(
            "MockExecution: Order {} placed and executed on Exchange.",
            order.id
        );
        Ok(())
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        let port = tokio::time::timeout(std::time::Duration::from_secs(2), self.portfolio.read())
            .await
            .map_err(|_| {
                anyhow::anyhow!("MockExecution: Deadlock detected acquiring Portfolio read lock")
            })?;
        Ok(port.clone())
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        let orders = self.orders.read().await;
        Ok(orders.clone())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        Ok(vec![])
    }

    async fn cancel_order(&self, _order_id: &str) -> Result<()> {
        Ok(())
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        Ok(self.order_update_sender.subscribe())
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
    async fn get_latest_timestamp(&self, _symbol: &str) -> Result<Option<i64>> {
        Ok(None)
    }
    async fn count_candles(&self, _symbol: &str, _start_ts: i64, _end_ts: i64) -> Result<usize> {
        Ok(0)
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
