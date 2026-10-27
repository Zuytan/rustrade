//! Shared handling of ambiguous order submissions.
//!
//! An order POST that times out or gets a 5xx may still have been accepted by the broker.
//! Such orders are never resent: their outcome is resolved by looking them up by client
//! order id, and stays unknown (`OrderSubmissionUnknown`) when the lookup is inconclusive.

use crate::domain::errors::OrderSubmissionUnknown;
use crate::domain::ports::ExecutionService;
use crate::domain::trading::types::Order;
use anyhow::Result;
use std::time::Duration;
use tracing::{info, warn};

/// Delay before looking up an ambiguous order, so the broker has time to register it.
pub const AMBIGUOUS_LOOKUP_DELAY: Duration = Duration::from_secs(1);

/// Internal marker: the broker may or may not have accepted the order.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AmbiguousSubmission(pub String);

/// True only when the request is known not to have reached the broker
/// (connection never established, or request never built).
pub fn is_definitely_not_sent(err: &reqwest_middleware::Error) -> bool {
    match err {
        reqwest_middleware::Error::Reqwest(e) => e.is_connect() || e.is_builder(),
        reqwest_middleware::Error::Middleware(_) => false,
    }
}

/// Converts a send error into a definitive failure or an `AmbiguousSubmission`.
pub fn classify_send_error(err: reqwest_middleware::Error, broker: &str) -> anyhow::Error {
    if is_definitely_not_sent(&err) {
        anyhow::Error::new(err).context(format!("{} order was not sent", broker))
    } else {
        AmbiguousSubmission(format!("{} order request failed: {}", broker, err)).into()
    }
}

/// Resolves an ambiguous submission by looking the order up at the broker.
///
/// - Found and live (or filled) → `Ok(())`: the order was accepted.
/// - Found rejected/canceled/expired with nothing executed → definitive error.
/// - Found canceled/expired after a partial fill → `OrderSubmissionUnknown`, so that
///   reconciliation books the executed quantity only.
/// - Not found, or lookup failed → `OrderSubmissionUnknown` (order must stay Pending).
pub async fn resolve_ambiguous_submission<S>(
    service: &S,
    order: &Order,
    reason: String,
    delay: Duration,
) -> Result<()>
where
    S: ExecutionService + ?Sized,
{
    warn!(
        "Order {} submission is ambiguous ({}). Looking it up before deciding.",
        order.id, reason
    );
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }

    match service.find_order_status(&order.id, &order.symbol).await {
        Ok(Some(state)) if state.is_dead_without_fill() => {
            anyhow::bail!(
                "Order {} was not accepted by the broker (status {:?}): {}",
                order.id,
                state.status,
                reason
            )
        }
        Ok(Some(state)) if state.is_terminal_partial_fill() => Err(OrderSubmissionUnknown {
            client_order_id: order.id.clone(),
            reason: format!(
                "{}; order ended {:?} after a partial fill of {}",
                reason, state.status, state.filled_qty
            ),
        }
        .into()),
        Ok(Some(state)) => {
            info!(
                "Order {} confirmed at broker with status {:?} despite: {}",
                order.id, state.status, reason
            );
            Ok(())
        }
        Ok(None) => Err(OrderSubmissionUnknown {
            client_order_id: order.id.clone(),
            reason: format!("{}; order not found at broker yet", reason),
        }
        .into()),
        Err(e) => Err(OrderSubmissionUnknown {
            client_order_id: order.id.clone(),
            reason: format!("{}; lookup failed: {}", reason, e),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::{BrokerOrderState, OrderUpdate};
    use crate::domain::trading::portfolio::Portfolio;
    use crate::domain::trading::types::{OrderSide, OrderStatus, OrderType};
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use tokio::sync::broadcast;

    struct LookupOnly(fn() -> Result<Option<BrokerOrderState>>);

    #[async_trait]
    impl ExecutionService for LookupOnly {
        async fn execute(&self, _order: &Order) -> Result<()> {
            anyhow::bail!("not used")
        }
        async fn get_portfolio(&self) -> Result<Portfolio> {
            anyhow::bail!("not used")
        }
        async fn get_today_orders(&self) -> Result<Vec<Order>> {
            anyhow::bail!("not used")
        }
        async fn get_open_orders(&self) -> Result<Vec<Order>> {
            anyhow::bail!("not used")
        }
        async fn cancel_order(&self, _order_id: &str, _symbol: &str) -> Result<()> {
            anyhow::bail!("not used")
        }
        async fn cancel_all_orders(&self) -> Result<()> {
            anyhow::bail!("not used")
        }
        async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
            anyhow::bail!("not used")
        }
        async fn find_order_status(
            &self,
            _client_order_id: &str,
            _symbol: &str,
        ) -> Result<Option<BrokerOrderState>> {
            (self.0)()
        }
    }

    fn order() -> Order {
        Order {
            id: "client-1".to_string(),
            symbol: "BTC/USD".to_string(),
            side: OrderSide::Buy,
            price: dec!(100),
            quantity: dec!(1),
            order_type: OrderType::Market,
            status: OrderStatus::Pending,
            timestamp: 0,
            correlation_id: None,
            stop_loss: None,
        }
    }

    fn st(status: OrderStatus, filled_qty: rust_decimal::Decimal) -> Option<BrokerOrderState> {
        Some(BrokerOrderState { status, filled_qty })
    }

    async fn resolve(lookup: fn() -> Result<Option<BrokerOrderState>>) -> Result<()> {
        resolve_ambiguous_submission(
            &LookupOnly(lookup),
            &order(),
            "timeout".to_string(),
            Duration::ZERO,
        )
        .await
    }

    fn is_unknown(err: &anyhow::Error) -> bool {
        err.downcast_ref::<OrderSubmissionUnknown>().is_some()
    }

    #[tokio::test]
    async fn test_found_live_order_is_accepted() {
        assert!(resolve(|| Ok(st(OrderStatus::New, dec!(0)))).await.is_ok());
        assert!(
            resolve(|| Ok(st(OrderStatus::Filled, dec!(1))))
                .await
                .is_ok()
        );
        assert!(
            resolve(|| Ok(st(OrderStatus::PartiallyFilled, dec!(0.5))))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_found_rejected_order_is_definitive_failure() {
        let err = resolve(|| Ok(st(OrderStatus::Rejected, dec!(0))))
            .await
            .expect_err("rejected at broker");
        assert!(!is_unknown(&err));
        let err = resolve(|| Ok(st(OrderStatus::Canceled, dec!(0))))
            .await
            .expect_err("canceled without fill");
        assert!(!is_unknown(&err));
    }

    #[tokio::test]
    async fn test_canceled_after_partial_fill_is_unknown() {
        // Something was executed: it must not be marked Rejected.
        let err = resolve(|| Ok(st(OrderStatus::Canceled, dec!(0.4))))
            .await
            .expect_err("partial fill then canceled");
        assert!(is_unknown(&err));
        let err = resolve(|| Ok(st(OrderStatus::Expired, dec!(0.4))))
            .await
            .expect_err("partial fill then expired");
        assert!(is_unknown(&err));
    }

    #[tokio::test]
    async fn test_not_found_is_unknown() {
        let err = resolve(|| Ok(None)).await.expect_err("not found");
        assert!(is_unknown(&err));
    }

    #[tokio::test]
    async fn test_lookup_failure_is_unknown() {
        let err = resolve(|| anyhow::bail!("broker down"))
            .await
            .expect_err("lookup failed");
        assert!(is_unknown(&err));
    }

    #[test]
    fn test_unknown_survives_anyhow_context() {
        let err: anyhow::Error = OrderSubmissionUnknown {
            client_order_id: "x".to_string(),
            reason: "r".to_string(),
        }
        .into();
        let wrapped = err.context("outer");
        assert!(is_unknown(&wrapped));
    }
}
