use super::reconciliation::RECONCILE_GRACE_MS;
use super::*;
use crate::domain::ports::{ExecutionService, OrderUpdate};
use crate::domain::trading::fee_model::ConstantFeeModel;
use anyhow::Result;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::mpsc;

struct MockExecService;
#[async_trait]
impl ExecutionService for MockExecService {
    async fn execute(&self, _order: &Order) -> Result<()> {
        Ok(())
    }
    async fn get_portfolio(&self) -> Result<Portfolio> {
        Ok(Portfolio::new())
    }
    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        Ok(Vec::new())
    }
    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        Ok(Vec::new())
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

struct FailExecService;
#[async_trait]
impl ExecutionService for FailExecService {
    async fn execute(&self, _order: &Order) -> Result<()> {
        Err(anyhow::anyhow!("Simulated Failure"))
    }
    async fn get_portfolio(&self) -> Result<Portfolio> {
        Err(anyhow::anyhow!("Simulated Failure"))
    }
    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        Err(anyhow::anyhow!("Simulated Failure"))
    }
    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        Err(anyhow::anyhow!("Simulated Failure"))
    }
    async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<()> {
        Err(anyhow::anyhow!("Simulated Failure"))
    }
    async fn cancel_all_orders(&self) -> Result<()> {
        Err(anyhow::anyhow!("Simulated Failure"))
    }
    async fn subscribe_order_updates(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
        Err(anyhow::anyhow!("Simulated Failure"))
    }
}

#[tokio::test]
async fn test_buy_updates_portfolio() {
    let (tx, rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));

    let fee_model = Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO));
    let mut executor = Executor::new(
        rx,
        portfolio.clone(),
        ExecutorDependencies {
            execution_service: Arc::new(MockExecService),
            repository: None,
            retry_config: RetryConfig::default(),
            health_service: Arc::new(ConnectionHealthService::new()),
            fee_model,
            agent_registry: Arc::new(
                crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                    crate::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
            candle_rx: None,
        },
    );
    tokio::spawn(async move { executor.run().await });

    let order = Order {
        id: "1".to_string(),
        symbol: "ABC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(2),
        order_type: crate::domain::trading::types::OrderType::Limit,
        status: crate::domain::trading::types::OrderStatus::New,
        timestamp: 0,
        correlation_id: None,
        stop_loss: None,
    };
    tx.send(order).await.expect("Failed to send order in test");

    // Allow update
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let p = portfolio.read().await;
    assert_eq!(p.cash, Decimal::from(800)); // 1000 - (100*2) - 0 fees
    assert!(p.positions.contains_key("ABC"), "Position ABC should exist");
    assert_eq!(p.positions.get("ABC").unwrap().quantity, Decimal::from(2));
}

#[tokio::test]
async fn test_failed_execution_does_not_update_portfolio() {
    let (tx, rx) = mpsc::channel(1);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));

    let fee_model = Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO));
    let mut executor = Executor::new(
        rx,
        portfolio.clone(),
        ExecutorDependencies {
            execution_service: Arc::new(FailExecService),
            repository: None,
            retry_config: RetryConfig::default(),
            health_service: Arc::new(ConnectionHealthService::new()),
            fee_model,
            agent_registry: Arc::new(
                crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                    crate::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
            candle_rx: None,
        },
    );
    tokio::spawn(async move { executor.run().await });

    let order = Order {
        id: "1".to_string(),
        symbol: "ABC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(2),
        order_type: crate::domain::trading::types::OrderType::Limit,
        status: crate::domain::trading::types::OrderStatus::New,
        timestamp: 0,
        correlation_id: None,
        stop_loss: None,
    };
    tx.send(order).await.expect("Failed to send order in test");

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let p = portfolio.read().await;
    assert_eq!(p.cash, Decimal::from(1000)); // Unchanged
}

struct CaptureExecService {
    executed: Arc<RwLock<Vec<Order>>>,
    cancelled: Arc<RwLock<Vec<String>>>,
}

#[async_trait]
impl ExecutionService for CaptureExecService {
    async fn execute(&self, order: &Order) -> Result<()> {
        self.executed.write().await.push(order.clone());
        Ok(())
    }
    async fn get_portfolio(&self) -> Result<Portfolio> {
        Ok(Portfolio::new())
    }
    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        Ok(Vec::new())
    }
    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        Ok(Vec::new())
    }
    async fn cancel_order(&self, order_id: &str, _symbol: &str) -> Result<()> {
        self.cancelled.write().await.push(order_id.to_string());
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
async fn test_executor_trailing_stop_loss() {
    let (tx, rx) = mpsc::channel(10);
    let (candle_tx, candle_rx) = tokio::sync::broadcast::channel(10);
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));

    let executed = Arc::new(RwLock::new(Vec::new()));
    let cancelled = Arc::new(RwLock::new(Vec::new()));
    let mock_exec = Arc::new(CaptureExecService {
        executed: executed.clone(),
        cancelled: cancelled.clone(),
    });

    let fee_model = Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO));
    let mut executor = Executor::new(
        rx,
        portfolio.clone(),
        ExecutorDependencies {
            execution_service: mock_exec,
            repository: None,
            retry_config: RetryConfig::default(),
            health_service: Arc::new(ConnectionHealthService::new()),
            fee_model,
            agent_registry: Arc::new(
                crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                    crate::infrastructure::observability::Metrics::new().unwrap(),
                ),
            ),
            candle_rx: Some(candle_rx),
        },
    );
    tokio::spawn(async move { executor.run().await });

    // 1. Submit BUY order with stop loss
    let order = Order {
        id: "buy_order_1".to_string(),
        symbol: "ABC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(2),
        order_type: crate::domain::trading::types::OrderType::Limit,
        status: crate::domain::trading::types::OrderStatus::New,
        timestamp: 0,
        correlation_id: None,
        stop_loss: Some(Decimal::from(90)),
    };
    tx.send(order).await.expect("Failed to send order");

    // Allow execution
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Verify that initial BUY and the initial Stop order were submitted
    let execs = executed.read().await;
    assert_eq!(execs.len(), 2);
    assert_eq!(execs[0].id, "buy_order_1");
    assert_eq!(
        execs[1].order_type,
        crate::domain::trading::types::OrderType::Stop
    );
    assert_eq!(execs[1].price, Decimal::from(90));
    assert_eq!(execs[1].side, OrderSide::Sell);
    let initial_stop_id = execs[1].id.clone();
    drop(execs);

    // 2. Send candle rising to 110 -> Stop should raise to 100
    let candle = crate::domain::trading::types::Candle {
        symbol: "ABC".to_string(),
        open: Decimal::from(100),
        high: Decimal::from(110),
        low: Decimal::from(100),
        close: Decimal::from(110),
        volume: Decimal::from(100),
        timestamp: 0,
    };
    candle_tx.send(candle).expect("Failed to send candle");

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Verify old stop order cancelled and new one placed
    let cancels = cancelled.read().await;
    assert_eq!(cancels.len(), 1);
    assert_eq!(cancels[0], initial_stop_id);

    let execs = executed.read().await;
    assert_eq!(execs.len(), 3);
    assert_eq!(
        execs[2].order_type,
        crate::domain::trading::types::OrderType::Stop
    );
    assert_eq!(execs[2].price, Decimal::from(100)); // 110 - 10 trailing distance
}

// ===== Reconciliation & ambiguous submission tests =====

use crate::domain::errors::OrderSubmissionUnknown;
use crate::domain::ports::BrokerOrderState;
use crate::domain::trading::types::OrderStatus;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

/// Repository that upserts by order id (like the SQLite repository).
#[derive(Default)]
struct UpsertRepo {
    orders: Mutex<HashMap<String, Order>>,
    fail_saves: AtomicBool,
}

impl UpsertRepo {
    fn with_orders(orders: Vec<Order>) -> Self {
        let repo = Self::default();
        for o in orders {
            repo.orders
                .lock()
                .expect("repo lock")
                .insert(o.id.clone(), o);
        }
        repo
    }

    fn status_of(&self, id: &str) -> Option<OrderStatus> {
        self.orders
            .lock()
            .expect("repo lock")
            .get(id)
            .map(|o| o.status)
    }
}

#[async_trait]
impl TradeRepository for UpsertRepo {
    async fn save(&self, trade: &Order) -> Result<()> {
        if self.fail_saves.load(Ordering::SeqCst) {
            anyhow::bail!("disk full");
        }
        self.orders
            .lock()
            .expect("repo lock")
            .insert(trade.id.clone(), trade.clone());
        Ok(())
    }
    async fn find_by_symbol(&self, symbol: &str) -> Result<Vec<Order>> {
        let orders = self.orders.lock().expect("repo lock");
        Ok(orders
            .values()
            .filter(|o| o.symbol == symbol)
            .cloned()
            .collect())
    }
    async fn find_by_status(&self, status: OrderStatus) -> Result<Vec<Order>> {
        let orders = self.orders.lock().expect("repo lock");
        Ok(orders
            .values()
            .filter(|o| o.status == status)
            .cloned()
            .collect())
    }
    async fn find_recent(&self, _limit: usize) -> Result<Vec<Order>> {
        self.get_all().await
    }
    async fn get_all(&self) -> Result<Vec<Order>> {
        Ok(self
            .orders
            .lock()
            .expect("repo lock")
            .values()
            .cloned()
            .collect())
    }
    async fn count(&self) -> Result<usize> {
        Ok(self.orders.lock().expect("repo lock").len())
    }
}

type Lookup = fn(&str) -> Result<Option<BrokerOrderState>>;

/// Broker state with the given executed quantity.
fn st(status: OrderStatus, filled_qty: u32) -> Option<BrokerOrderState> {
    Some(BrokerOrderState {
        status,
        filled_qty: Decimal::from(filled_qty),
    })
}

/// Execution service with scripted `execute` and `find_order_status` behavior.
struct ScriptedExecService {
    execute_unknown: bool,
    lookup: Lookup,
    execute_calls: AtomicUsize,
}

impl ScriptedExecService {
    fn new(execute_unknown: bool, lookup: Lookup) -> Self {
        Self {
            execute_unknown,
            lookup,
            execute_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ExecutionService for ScriptedExecService {
    async fn execute(&self, order: &Order) -> Result<()> {
        self.execute_calls.fetch_add(1, Ordering::SeqCst);
        if self.execute_unknown {
            return Err(OrderSubmissionUnknown {
                client_order_id: order.id.clone(),
                reason: "timeout".to_string(),
            }
            .into());
        }
        Ok(())
    }
    async fn get_portfolio(&self) -> Result<Portfolio> {
        Ok(Portfolio::new())
    }
    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        anyhow::bail!("reconciliation must not rely on today's orders")
    }
    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        anyhow::bail!("reconciliation must not rely on open orders")
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
    async fn find_order_status(
        &self,
        client_order_id: &str,
        _symbol: &str,
    ) -> Result<Option<BrokerOrderState>> {
        (self.lookup)(client_order_id)
    }
}

fn pending_order(id: &str, timestamp_ms: i64) -> Order {
    Order {
        id: id.to_string(),
        symbol: "ABC".to_string(),
        side: OrderSide::Buy,
        price: Decimal::from(100),
        quantity: Decimal::from(2),
        order_type: crate::domain::trading::types::OrderType::Limit,
        status: OrderStatus::Pending,
        timestamp: timestamp_ms,
        correlation_id: None,
        stop_loss: None,
    }
}

fn executor_with(
    exec: Arc<ScriptedExecService>,
    repo: Arc<UpsertRepo>,
    portfolio: Arc<RwLock<Portfolio>>,
) -> (Executor, mpsc::Sender<Order>) {
    let (tx, rx) = mpsc::channel(10);
    let executor = Executor::new(
        rx,
        portfolio,
        ExecutorDependencies {
            execution_service: exec,
            repository: Some(repo),
            retry_config: RetryConfig::default(),
            health_service: Arc::new(ConnectionHealthService::new()),
            fee_model: Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO)),
            agent_registry: Arc::new(
                crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                    crate::infrastructure::observability::Metrics::new().expect("metrics"),
                ),
            ),
            candle_rx: None,
        },
    );
    (executor, tx)
}

fn old_ts() -> i64 {
    chrono::Utc::now().timestamp_millis() - 10 * RECONCILE_GRACE_MS
}

#[tokio::test]
async fn test_reconcile_applies_broker_status_and_rejects_missing_old_orders() {
    let repo = Arc::new(UpsertRepo::with_orders(vec![
        pending_order("filled", old_ts()),
        pending_order("missing", old_ts()),
    ]));
    let exec = Arc::new(ScriptedExecService::new(false, |id| match id {
        "filled" => Ok(st(OrderStatus::Filled, 2)),
        _ => Ok(None),
    }));
    let (executor, _tx) =
        executor_with(exec, repo.clone(), Arc::new(RwLock::new(Portfolio::new())));

    let resolved = executor.reconcile_pending().await.expect("reconcile ok");
    assert_eq!(resolved, 2);
    assert_eq!(repo.status_of("filled"), Some(OrderStatus::Filled));
    assert_eq!(repo.status_of("missing"), Some(OrderStatus::Rejected));
}

#[tokio::test]
async fn test_reconcile_keeps_young_missing_order_pending() {
    let now = chrono::Utc::now().timestamp_millis();
    let repo = Arc::new(UpsertRepo::with_orders(vec![pending_order("young", now)]));
    let exec = Arc::new(ScriptedExecService::new(false, |_| Ok(None)));
    let (executor, _tx) =
        executor_with(exec, repo.clone(), Arc::new(RwLock::new(Portfolio::new())));

    let resolved = executor.reconcile_pending().await.expect("reconcile ok");
    assert_eq!(resolved, 0);
    assert_eq!(repo.status_of("young"), Some(OrderStatus::Pending));
}

#[tokio::test]
async fn test_reconcile_lookup_failure_keeps_order_pending_and_errors() {
    let repo = Arc::new(UpsertRepo::with_orders(vec![
        pending_order("unreachable", old_ts()),
        pending_order("filled", old_ts()),
    ]));
    let exec = Arc::new(ScriptedExecService::new(false, |id| match id {
        "filled" => Ok(st(OrderStatus::Filled, 2)),
        _ => anyhow::bail!("broker unreachable"),
    }));
    let (executor, _tx) =
        executor_with(exec, repo.clone(), Arc::new(RwLock::new(Portfolio::new())));

    assert!(executor.reconcile_pending().await.is_err());
    assert_eq!(repo.status_of("unreachable"), Some(OrderStatus::Pending));
    // Other orders are still reconciled.
    assert_eq!(repo.status_of("filled"), Some(OrderStatus::Filled));
}

#[tokio::test]
async fn test_reconcile_propagates_save_failure() {
    let repo = Arc::new(UpsertRepo::with_orders(vec![pending_order("o1", old_ts())]));
    repo.fail_saves.store(true, Ordering::SeqCst);
    let exec = Arc::new(ScriptedExecService::new(false, |_| {
        Ok(st(OrderStatus::Filled, 2))
    }));
    let (executor, _tx) = executor_with(exec, repo, Arc::new(RwLock::new(Portfolio::new())));

    assert!(executor.reconcile_pending().await.is_err());
}

#[tokio::test]
async fn test_orders_refused_until_reconciled() {
    let repo = Arc::new(UpsertRepo::with_orders(vec![pending_order(
        "stuck",
        old_ts(),
    )]));
    let exec = Arc::new(ScriptedExecService::new(false, |id| match id {
        "stuck" => anyhow::bail!("broker unreachable"),
        _ => Ok(None),
    }));
    let portfolio = Arc::new(RwLock::new(Portfolio::new()));
    let (mut executor, tx) = executor_with(exec.clone(), repo.clone(), portfolio);
    tokio::spawn(async move { executor.run().await });

    let mut order = pending_order("new_order", 0);
    order.status = OrderStatus::New;
    tx.send(order).await.expect("send order");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 0);
    assert_eq!(repo.status_of("new_order"), Some(OrderStatus::Rejected));
    assert_eq!(repo.status_of("stuck"), Some(OrderStatus::Pending));

    // Exits stay possible: a Sell is executed even while unreconciled.
    let mut exit = pending_order("exit_order", chrono::Utc::now().timestamp_millis());
    exit.side = OrderSide::Sell;
    exit.status = OrderStatus::New;
    tx.send(exit).await.expect("send exit");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 1);
    assert_eq!(repo.status_of("exit_order"), Some(OrderStatus::New));
}

#[tokio::test]
async fn test_unknown_submission_keeps_order_pending_and_portfolio_unchanged() {
    let repo = Arc::new(UpsertRepo::default());
    let exec = Arc::new(ScriptedExecService::new(true, |_| Ok(None)));
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));
    let (mut executor, tx) = executor_with(exec.clone(), repo.clone(), portfolio.clone());
    tokio::spawn(async move { executor.run().await });

    // Fresh timestamp: within the grace period, the periodic pass must not reject it.
    let mut order = pending_order("ambiguous", chrono::Utc::now().timestamp_millis());
    order.status = OrderStatus::New;
    tx.send(order).await.expect("send order");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 1);
    assert_eq!(repo.status_of("ambiguous"), Some(OrderStatus::Pending));
    let p = portfolio.read().await;
    assert_eq!(p.cash, Decimal::from(1000));
    assert!(!p.positions.contains_key("ABC"));
}

#[tokio::test]
async fn test_unknown_buy_confirmed_filled_updates_portfolio_and_places_stop() {
    let repo = Arc::new(UpsertRepo::default());
    let exec = Arc::new(ScriptedExecService::new(true, |id| match id {
        "ambiguous_buy" => Ok(st(OrderStatus::Filled, 2)),
        _ => Ok(None),
    }));
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));
    let (executor, _tx) = executor_with(exec.clone(), repo.clone(), portfolio.clone());
    assert!(executor.try_reconcile().await);

    let mut order = pending_order("ambiguous_buy", chrono::Utc::now().timestamp_millis());
    order.stop_loss = Some(Decimal::from(90));
    executor.handle_order(order).await;
    assert_eq!(repo.status_of("ambiguous_buy"), Some(OrderStatus::Pending));
    assert!(executor.active_trailing_stops.read().await.is_empty());

    // Reconciliation confirms the fill: the skipped accepted path now runs.
    assert_eq!(executor.reconcile_pending().await.expect("reconcile"), 1);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert_eq!(repo.status_of("ambiguous_buy"), Some(OrderStatus::Filled));
    let p = portfolio.read().await;
    assert_eq!(p.cash, Decimal::from(800)); // 1000 - 100 * 2
    assert!(p.positions.contains_key("ABC"));
    drop(p);
    assert!(
        executor
            .active_trailing_stops
            .read()
            .await
            .contains_key("ABC")
    );
    // Buy + initial stop order submission
    assert_eq!(exec.execute_calls.load(Ordering::SeqCst), 2);
    assert!(executor.unknown_orders.read().await.is_empty());
}

#[tokio::test]
async fn test_unknown_order_confirmed_rejected_leaves_portfolio_unchanged() {
    let repo = Arc::new(UpsertRepo::default());
    let exec = Arc::new(ScriptedExecService::new(true, |_| {
        Ok(st(OrderStatus::Rejected, 0))
    }));
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));
    let (executor, _tx) = executor_with(exec, repo.clone(), portfolio.clone());
    assert!(executor.try_reconcile().await);

    let mut order = pending_order("ambiguous_buy", chrono::Utc::now().timestamp_millis());
    order.stop_loss = Some(Decimal::from(90));
    executor.handle_order(order).await;
    executor.reconcile_pending().await.expect("reconcile");

    assert_eq!(repo.status_of("ambiguous_buy"), Some(OrderStatus::Rejected));
    assert_eq!(portfolio.read().await.cash, Decimal::from(1000));
    assert!(executor.active_trailing_stops.read().await.is_empty());
    assert!(executor.unknown_orders.read().await.is_empty());
}

#[tokio::test]
async fn test_unknown_buy_canceled_after_partial_fill_books_filled_quantity() {
    let repo = Arc::new(UpsertRepo::default());
    let exec = Arc::new(ScriptedExecService::new(true, |_| {
        Ok(st(OrderStatus::Canceled, 1))
    }));
    let mut port = Portfolio::new();
    port.cash = Decimal::from(1000);
    let portfolio = Arc::new(RwLock::new(port));
    let (executor, _tx) = executor_with(exec, repo.clone(), portfolio.clone());
    assert!(executor.try_reconcile().await);

    let mut order = pending_order("partial", chrono::Utc::now().timestamp_millis());
    order.stop_loss = Some(Decimal::from(90));
    executor.handle_order(order).await;
    executor.reconcile_pending().await.expect("reconcile");
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // 1 of 2 executed: only that quantity is booked and protected.
    assert_eq!(repo.status_of("partial"), Some(OrderStatus::Canceled));
    let p = portfolio.read().await;
    assert_eq!(p.cash, Decimal::from(900));
    assert_eq!(
        p.positions.get("ABC").map(|pos| pos.quantity),
        Some(Decimal::from(1))
    );
    drop(p);
    let stops = executor.active_trailing_stops.read().await;
    assert_eq!(stops.get("ABC").map(|s| s.quantity), Some(Decimal::from(1)));
}
