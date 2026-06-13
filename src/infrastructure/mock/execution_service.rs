use crate::domain::ports::{ExecutionService, OrderUpdate};
use crate::domain::trading::fee_model::{ConstantFeeModel, FeeModel};
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Order, OrderSide, OrderStatus};
use crate::infrastructure::simulation::latency_model::{LatencyModel, ZeroLatency};
use crate::infrastructure::simulation::slippage_model::{SlippageModel, ZeroSlippage};
use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::broadcast;
use tracing::info;

pub struct MockExecutionService {
    portfolio: Arc<RwLock<Portfolio>>,
    orders: Arc<RwLock<Vec<Order>>>,
    fee_model: Arc<dyn FeeModel>,
    // New simulation models
    latency_model: Arc<dyn LatencyModel>,
    slippage_model: Arc<dyn SlippageModel>,
    order_update_sender: broadcast::Sender<OrderUpdate>,
}

impl MockExecutionService {
    pub fn new(portfolio: Arc<RwLock<Portfolio>>) -> Self {
        {
            if let Ok(mut guard) = portfolio.try_write() {
                guard.synchronized = true;
            } else {
                tracing::warn!(
                    "MockExecutionService: Could not acquire lock to set synchronized=true. Assuming handled elsewhere."
                );
            }
        }
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            fee_model: Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO)),
            latency_model: Arc::new(ZeroLatency),
            slippage_model: Arc::new(ZeroSlippage),
            order_update_sender: broadcast::channel(100).0,
        }
    }

    pub fn with_simulation_models(
        portfolio: Arc<RwLock<Portfolio>>,
        fee_model: Arc<dyn FeeModel>,
        latency_model: Arc<dyn LatencyModel>,
        slippage_model: Arc<dyn SlippageModel>,
    ) -> Self {
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            fee_model,
            latency_model,
            slippage_model,
            order_update_sender: broadcast::channel(100).0,
        }
    }

    pub fn with_costs(portfolio: Arc<RwLock<Portfolio>>, fee_model: Arc<dyn FeeModel>) -> Self {
        Self {
            portfolio,
            orders: Arc::new(RwLock::new(Vec::new())),
            fee_model,
            latency_model: Arc::new(ZeroLatency),
            slippage_model: Arc::new(ZeroSlippage),
            order_update_sender: broadcast::channel(100).0,
        }
    }
}

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute(&self, order: &Order) -> Result<()> {
        info!("MockExecution: Placing order {}...", order.id);

        // Simulate Network Latency
        let latency = self.latency_model.next_latency();
        if !latency.is_zero() {
            tracing::debug!("MockExecution: Simulating network latency of {:?}", latency);
            tokio::time::sleep(latency).await;
        }

        let mut port =
            tokio::time::timeout(std::time::Duration::from_secs(2), self.portfolio.write())
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "MockExecution: Deadlock detected acquiring Portfolio write lock"
                    )
                })?;

        // Calculate Execution Price with Slippage
        let execution_price =
            self.slippage_model
                .calculate_execution_price(order.price, order.quantity, order.side);

        // Calculate commissions
        let costs = self
            .fee_model
            .calculate_cost(order.quantity, execution_price, order.side);

        let commission = costs.fee;

        // Slippage Impact calc for logging
        let price_impact = (execution_price - order.price).abs() * order.quantity;

        // Total cost value (base value)
        let cost = execution_price * order.quantity;

        info!(
            "MockExecution: Order {} - Price: {} -> {}, Slippage Impact: ${:.4}, Commission: ${:.4}",
            order.id, order.price, execution_price, price_impact, commission
        );

        match order.side {
            OrderSide::Buy => {
                let total_needed = cost + commission;
                if port.cash < total_needed {
                    // Reduce quantity to what cash allows (no margin / no negative cash)
                    let available_for_cost = (port.cash - commission).max(Decimal::ZERO);
                    let affordable_qty = available_for_cost
                        .checked_div(execution_price)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp(4);
                    if affordable_qty <= Decimal::ZERO {
                        info!(
                            "MockExecution: Order {} REJECTED — insufficient cash (need ${}, have ${})",
                            order.id, total_needed, port.cash
                        );
                        return Err(anyhow::anyhow!(
                            "Insufficient cash: need {}, have {}",
                            total_needed,
                            port.cash
                        ));
                    }
                    // Execute with reduced quantity
                    let reduced_cost = execution_price * affordable_qty;
                    let reduced_commission = self
                        .fee_model
                        .calculate_cost(affordable_qty, execution_price, order.side)
                        .fee;
                    info!(
                        "MockExecution: Order {} reduced qty {} -> {} (cash ${} < needed ${})",
                        order.id, order.quantity, affordable_qty, port.cash, total_needed
                    );
                    port.cash -= reduced_cost + reduced_commission;
                    let pos = port.positions.entry(order.symbol.clone()).or_insert(
                        crate::domain::trading::portfolio::Position {
                            symbol: order.symbol.clone(),
                            quantity: Decimal::ZERO,
                            average_price: Decimal::ZERO,
                        },
                    );
                    let total_qty = pos.quantity + affordable_qty;
                    let total_cost_pos = (pos.quantity * pos.average_price) + reduced_cost;
                    if total_qty > Decimal::ZERO {
                        pos.average_price = total_cost_pos
                            .checked_div(total_qty)
                            .unwrap_or(Decimal::ZERO);
                    }
                    pos.quantity = total_qty;
                } else {
                    port.cash -= total_needed;
                    let pos = port.positions.entry(order.symbol.clone()).or_insert(
                        crate::domain::trading::portfolio::Position {
                            symbol: order.symbol.clone(),
                            quantity: Decimal::ZERO,
                            average_price: Decimal::ZERO,
                        },
                    );
                    let total_qty = pos.quantity + order.quantity;
                    let total_cost_pos = (pos.quantity * pos.average_price) + cost;
                    if total_qty > Decimal::ZERO {
                        pos.average_price = total_cost_pos
                            .checked_div(total_qty)
                            .unwrap_or(Decimal::ZERO);
                    }
                    pos.quantity = total_qty;
                }
            }
            OrderSide::Sell => {
                // Prevent selling more than we hold
                let current_qty = port
                    .positions
                    .get(&order.symbol)
                    .map(|p| p.quantity)
                    .unwrap_or(Decimal::ZERO);
                let sell_qty = order.quantity.min(current_qty);
                if sell_qty <= Decimal::ZERO {
                    info!(
                        "MockExecution: Sell order {} REJECTED — no position to sell",
                        order.id
                    );
                    return Err(anyhow::anyhow!("No position to sell for {}", order.symbol));
                }
                let sell_proceeds = execution_price * sell_qty;
                let sell_commission = self
                    .fee_model
                    .calculate_cost(sell_qty, execution_price, order.side)
                    .fee;

                // Calculate hold time and funding cost
                let last_buy = if let Ok(orders) =
                    tokio::time::timeout(std::time::Duration::from_millis(500), self.orders.read())
                        .await
                {
                    orders
                        .iter()
                        .rev()
                        .find(|o| o.symbol == order.symbol && o.side == OrderSide::Buy)
                        .cloned()
                } else {
                    None
                };

                let hold_time_ms = if let Some(buy_order) = last_buy {
                    (order.timestamp - buy_order.timestamp).max(0)
                } else {
                    0
                };
                use rust_decimal_macros::dec;
                let hold_time_hours = rust_decimal::Decimal::from(hold_time_ms) / dec!(3600000.0);

                let funding_cost = self.fee_model.calculate_funding_cost(
                    sell_qty,
                    execution_price,
                    hold_time_hours,
                );

                port.cash += sell_proceeds - sell_commission - funding_cost;
                let pos = port.positions.entry(order.symbol.clone()).or_insert(
                    crate::domain::trading::portfolio::Position {
                        symbol: order.symbol.clone(),
                        quantity: Decimal::ZERO,
                        average_price: Decimal::ZERO,
                    },
                );
                pos.quantity -= sell_qty;
            }
        }

        self.orders.write().await.push(order.clone());

        let update_timestamp = chrono::DateTime::from_timestamp(
            order.timestamp / 1000,
            ((order.timestamp % 1000) * 1_000_000) as u32,
        )
        .unwrap_or_else(chrono::Utc::now);

        let _ = self.order_update_sender.send(OrderUpdate {
            order_id: order.id.clone(),
            client_order_id: order.id.clone(),
            symbol: order.symbol.clone(),
            side: order.side,
            status: OrderStatus::Filled,
            filled_qty: order.quantity,
            filled_avg_price: Some(execution_price),
            timestamp: update_timestamp,
            fees: Some(commission),
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

    async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<()> {
        Ok(())
    }

    async fn cancel_all_orders(&self) -> Result<()> {
        info!("MockExecution: Cancelling all orders");
        Ok(())
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        Ok(self.order_update_sender.subscribe())
    }
}
