//! Binance Execution Service
//!
//! Provides order execution functionality for Binance crypto exchange including:
//! - Order placement (Market, Limit, Stop orders)
//! - Portfolio/account retrieval
//! - Open orders management
//! - HMAC-SHA256 request signing

use crate::domain::ports::{ExecutionService, OrderUpdate};
use crate::domain::trading::portfolio::{Portfolio, Position};
use crate::domain::trading::types::{
    Order, OrderSide, OrderStatus, OrderType, denormalize_crypto_symbol, normalize_crypto_symbol,
};
use crate::infrastructure::core::circuit_breaker::CircuitBreaker;
use crate::infrastructure::core::http_client_factory::HttpClientFactory;
use crate::infrastructure::core::order_submission::{
    AMBIGUOUS_LOOKUP_DELAY, AmbiguousSubmission, classify_send_error, resolve_ambiguous_submission,
};
use anyhow::{Context, Result};
use async_trait::async_trait;

use hmac::{Hmac, Mac};
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, instrument, warn};

pub struct BinanceExecutionService {
    client: ClientWithMiddleware,
    /// No-retry client used only to submit orders.
    write_client: ClientWithMiddleware,
    api_key: String,
    api_secret: String,
    base_url: String,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl BinanceExecutionService {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        let client = HttpClientFactory::create_client();
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            "BinanceExecution",
            5,
            3,
            std::time::Duration::from_secs(60),
        ));

        Self {
            client,
            write_client: HttpClientFactory::create_write_client(),
            api_key,
            api_secret,
            base_url,
            circuit_breaker,
        }
    }

    /// Generate HMAC-SHA256 signature for Binance API requests
    fn sign_request(&self, query_string: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

#[async_trait]
impl ExecutionService for BinanceExecutionService {
    #[instrument(skip(self, order), fields(symbol = %order.symbol, side = ?order.side, correlation_id = ?order.correlation_id))]
    async fn execute(&self, order: &Order) -> Result<()> {
        let correlation_id = order.correlation_id.clone();
        let api_symbol = denormalize_crypto_symbol(&order.symbol);
        let side = order.side;
        let order_type = order.order_type;
        let quantity = order.quantity;
        let order_id = order.id.clone();
        let price = order.price;

        let result = self
            .circuit_breaker
            .call(async {
                let side_str = match side {
                    OrderSide::Buy => "BUY",
                    OrderSide::Sell => "SELL",
                };

                let order_type_str = match order_type {
                    OrderType::Market => "MARKET",
                    OrderType::Limit => "LIMIT",
                    OrderType::Stop => "STOP_LOSS",
                    OrderType::StopLimit => "STOP_LOSS_LIMIT",
                };

                let timestamp = chrono::Utc::now().timestamp_millis();

                let mut params = vec![
                    ("symbol", api_symbol.clone()),
                    ("side", side_str.to_string()),
                    ("type", order_type_str.to_string()),
                    ("quantity", quantity.to_string()),
                    ("newClientOrderId", order_id.clone()),
                    ("timestamp", timestamp.to_string()),
                ];

                if let OrderType::Limit = order_type
                    && price > Decimal::ZERO
                {
                    params.push(("price", price.to_string()));
                    params.push(("timeInForce", "GTC".to_string()));
                }

                let query_string: String = params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&");

                let signature = self.sign_request(&query_string);
                let signed_query = format!("{}&signature={}", query_string, signature);

                let url = format!("{}/api/v3/order?{}", self.base_url, signed_query);

                // No automatic retry here: a resent POST could place the order twice.
                let response = self
                    .write_client
                    .post(&url)
                    .header("X-MBX-APIKEY", &self.api_key)
                    .send()
                    .await
                    .map_err(|e| classify_send_error(e, "Binance"))?;

                let status = response.status();
                if !status.is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    if status.is_server_error() {
                        return Err(AmbiguousSubmission(format!(
                            "Binance returned {}: {}",
                            status, error_text
                        ))
                        .into());
                    }
                    if is_duplicate_order_rejection(&error_text) {
                        return Err(AmbiguousSubmission(format!(
                            "Binance reports duplicate order: {}",
                            error_text
                        ))
                        .into());
                    }
                    anyhow::bail!("Binance order placement failed: {}", error_text);
                }

                // 2xx means the order is accepted, even if the body cannot be parsed.
                match response.json::<serde_json::Value>().await {
                    Ok(response_json) => info!(
                        "Binance order placed successfully: {:?} with correlation_id: {:?}",
                        response_json, correlation_id
                    ),
                    Err(e) => warn!(
                        "Binance order {} accepted but response unreadable: {}",
                        order_id, e
                    ),
                }

                Ok(())
            })
            .await;

        match result {
            Ok(()) => Ok(()),
            Err(crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Open(msg)) => {
                Err(anyhow::anyhow!(
                    "Binance Execution circuit breaker open: {}",
                    msg
                ))
            }
            Err(crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Inner(
                inner,
            )) => match inner.downcast::<AmbiguousSubmission>() {
                // Resolved outside the circuit breaker so an open breaker cannot hide the lookup.
                Ok(ambiguous) => {
                    resolve_ambiguous_submission(self, order, ambiguous.0, AMBIGUOUS_LOOKUP_DELAY)
                        .await
                }
                Err(definitive) => Err(definitive),
            },
        }
    }

    async fn find_order_status(
        &self,
        client_order_id: &str,
        symbol: &str,
    ) -> Result<Option<OrderStatus>> {
        let api_symbol = denormalize_crypto_symbol(symbol);
        let timestamp = chrono::Utc::now().timestamp_millis();
        let query_string = format!(
            "symbol={}&origClientOrderId={}&timestamp={}",
            api_symbol, client_order_id, timestamp
        );
        let signature = self.sign_request(&query_string);
        let url = format!(
            "{}/api/v3/order?{}&signature={}",
            self.base_url, query_string, signature
        );

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to look up Binance order by client order id")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            if binance_error_code(&body) == Some(BINANCE_ORDER_DOES_NOT_EXIST) {
                return Ok(None);
            }
            anyhow::bail!(
                "Binance order lookup for {} failed: {} - {}",
                client_order_id,
                status,
                body
            );
        }

        #[derive(Debug, Deserialize)]
        struct BinanceOrderStatus {
            status: String,
        }
        let order: BinanceOrderStatus =
            serde_json::from_str(&body).context("Failed to parse Binance order lookup response")?;
        map_binance_status(&order.status).map(Some)
    }

    async fn get_portfolio(&self) -> Result<Portfolio> {
        self.circuit_breaker
            .call(async move {
                let timestamp = chrono::Utc::now().timestamp_millis();
                let query_string = format!("timestamp={}", timestamp);
                let signature = self.sign_request(&query_string);
                let signed_query = format!("{}&signature={}", query_string, signature);

                let url = format!("{}/api/v3/account?{}", self.base_url, signed_query);

                let response = self
                    .client
                    .get(&url)
                    .header("X-MBX-APIKEY", &self.api_key)
                    .send()
                    .await
                    .context("Failed to fetch account from Binance")?;

                let status = response.status();
                if !status.is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    warn!(
                        "Binance account fetch failed - Status: {}, URL: {}, Response: {}",
                        status, url, error_text
                    );
                    anyhow::bail!("Binance account fetch failed: {} - {}", status, error_text);
                }

                #[derive(Debug, Deserialize)]
                struct Balance {
                    asset: String,
                    free: String,
                    locked: String,
                }

                #[derive(Debug, Deserialize)]
                struct Account {
                    balances: Vec<Balance>,
                }

                let account: Account = response.json().await?;
                let mut portfolio = Portfolio::new();

                for b in account.balances {
                    let free = b.free.parse::<Decimal>().unwrap_or(Decimal::ZERO);
                    let locked = b.locked.parse::<Decimal>().unwrap_or(Decimal::ZERO);
                    let total = free + locked;

                    if total > Decimal::ZERO {
                        if b.asset == "USDT" || b.asset == "USD" {
                            portfolio.cash += total;
                        } else {
                            // Assuming symbols are normalized as ASSET/USDT
                            let symbol = format!("{}/USDT", b.asset);
                            portfolio.positions.insert(
                                symbol.clone(),
                                Position {
                                    symbol,
                                    quantity: total,
                                    average_price: Decimal::ZERO, // Need to fetch average if possible
                                },
                            );
                        }
                    }
                }

                portfolio.synchronized = true;
                Ok(portfolio)
            })
            .await
            .map_err(|e| match e {
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Open(msg) => {
                    anyhow::anyhow!("Binance Execution circuit breaker open: {}", msg)
                }
                crate::infrastructure::core::circuit_breaker::CircuitBreakerError::Inner(inner) => {
                    inner
                }
            })
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        anyhow::bail!(
            "BinanceExecutionService::get_today_orders is not implemented - requires symbol parameter"
        );
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let query_string = format!("timestamp={}", timestamp);
        let signature = self.sign_request(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = format!("{}/api/v3/openOrders?{}", self.base_url, signed_query);

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("Failed to fetch open orders from Binance")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance open orders fetch failed: {}", error_text);
        }

        #[derive(Debug, Deserialize)]
        struct BinanceOrder {
            symbol: String,
            #[serde(rename = "orderId")]
            order_id: i64,
            side: String,
            #[serde(rename = "type")]
            order_type: String,
            #[serde(rename = "origQty")]
            orig_qty: String,
            price: String,
        }

        let binance_orders: Vec<BinanceOrder> = response.json().await?;

        let orders: Vec<Order> = binance_orders
            .into_iter()
            .filter_map(|bo| {
                let symbol = normalize_crypto_symbol(&bo.symbol).ok()?;
                let side = match bo.side.as_str() {
                    "BUY" => OrderSide::Buy,
                    "SELL" => OrderSide::Sell,
                    _ => return None,
                };
                let order_type = match bo.order_type.as_str() {
                    "MARKET" => OrderType::Market,
                    "LIMIT" => OrderType::Limit,
                    _ => return None,
                };
                let quantity =
                    Decimal::from_f64_retain(bo.orig_qty.parse().ok()?).unwrap_or(Decimal::ZERO);
                let price = if order_type == OrderType::Limit {
                    Decimal::from_f64_retain(bo.price.parse().ok()?).unwrap_or(Decimal::ZERO)
                } else {
                    Decimal::ZERO
                };

                Some(Order {
                    id: bo.order_id.to_string(),
                    symbol,
                    side,
                    order_type,
                    quantity,
                    price,
                    status: crate::domain::trading::types::OrderStatus::New,
                    timestamp: chrono::Utc::now().timestamp(),
                    correlation_id: None,
                    stop_loss: None,
                })
            })
            .collect();

        Ok(orders)
    }

    async fn cancel_order(&self, order_id: &str, symbol: &str) -> Result<()> {
        let api_symbol = denormalize_crypto_symbol(symbol);
        let timestamp = chrono::Utc::now().timestamp_millis();
        let query_string = format!(
            "symbol={}&orderId={}&timestamp={}",
            api_symbol, order_id, timestamp
        );
        let signature = self.sign_request(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = format!("{}/api/v3/order?{}", self.base_url, signed_query);

        let response = self
            .client
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context(format!("Failed to cancel order {}", order_id))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance cancel order failed: {}", error_text);
        }

        info!("Binance order {} cancelled successfully.", order_id);
        Ok(())
    }

    async fn cancel_all_orders(&self) -> Result<()> {
        // 1. Fetch all open orders
        let open_orders = self.get_open_orders().await?;
        if open_orders.is_empty() {
            return Ok(());
        }

        // 2. Identify unique symbols
        let symbols: std::collections::HashSet<String> =
            open_orders.into_iter().map(|o| o.symbol).collect();

        // 3. Cancel for each symbol
        for symbol in symbols {
            // Need to denormalize if normalized? get_open_orders returns normalized?
            // Yes, get_open_orders returns normalized. cancel endpoint needs API format.
            let api_symbol = denormalize_crypto_symbol(&symbol);

            let timestamp = chrono::Utc::now().timestamp_millis();
            let query_string = format!("symbol={}&timestamp={}", api_symbol, timestamp);
            let signature = self.sign_request(&query_string);
            let signed_query = format!("{}&signature={}", query_string, signature);

            let url = format!("{}/api/v3/openOrders?{}", self.base_url, signed_query);

            let response = self
                .client
                .delete(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await
                .context(format!("Failed to cancel orders for {}", symbol))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                warn!("Binance cancel all for {} failed: {}", symbol, error_text);
                // Continue to next symbol? Or fail?
                // Best effort: log and continue
            } else {
                info!("Binance orders for {} cancelled.", symbol);
            }
        }
        Ok(())
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        anyhow::bail!(
            "BinanceExecutionService::subscribe_order_updates is not implemented (requires User Data Stream)"
        );
    }
}

/// Binance NEW_ORDER_REJECTED code. Shared by many rejections (e.g. insufficient
/// balance); only its "Duplicate order sent." message means the id was already used.
const BINANCE_DUPLICATE_ORDER: i64 = -2010;
/// Binance error code returned when looking up an order that does not exist.
const BINANCE_ORDER_DOES_NOT_EXIST: i64 = -2013;

/// Extracts the `code` field from a Binance JSON error body.
fn binance_error_code(body: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("code")?
        .as_i64()
}

/// True only for a rejection caused by a reused `newClientOrderId`.
fn is_duplicate_order_rejection(body: &str) -> bool {
    binance_error_code(body) == Some(BINANCE_DUPLICATE_ORDER)
        && body.to_lowercase().contains("duplicate order")
}

/// Maps a Binance order status string to the domain status.
/// Unknown values are an error, never a default.
fn map_binance_status(status: &str) -> Result<OrderStatus> {
    Ok(match status {
        "NEW" => OrderStatus::New,
        "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
        "FILLED" => OrderStatus::Filled,
        "CANCELED" => OrderStatus::Canceled,
        "PENDING_CANCEL" => OrderStatus::PendingCancel,
        "PENDING_NEW" => OrderStatus::PendingNew,
        "REJECTED" => OrderStatus::Rejected,
        "EXPIRED" | "EXPIRED_IN_MATCH" => OrderStatus::Expired,
        other => anyhow::bail!("Unknown Binance order status: {}", other),
    })
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn test_map_binance_status_known_values() {
        let cases = [
            ("NEW", OrderStatus::New),
            ("PARTIALLY_FILLED", OrderStatus::PartiallyFilled),
            ("FILLED", OrderStatus::Filled),
            ("CANCELED", OrderStatus::Canceled),
            ("PENDING_CANCEL", OrderStatus::PendingCancel),
            ("PENDING_NEW", OrderStatus::PendingNew),
            ("REJECTED", OrderStatus::Rejected),
            ("EXPIRED", OrderStatus::Expired),
            ("EXPIRED_IN_MATCH", OrderStatus::Expired),
        ];
        for (raw, expected) in cases {
            assert_eq!(map_binance_status(raw).expect(raw), expected);
        }
    }

    #[test]
    fn test_map_binance_status_unknown_is_error() {
        assert!(map_binance_status("new").is_err());
        assert!(map_binance_status("SOMETHING").is_err());
    }

    #[test]
    fn test_binance_error_code_parsing() {
        assert_eq!(
            binance_error_code(r#"{"code":-2010,"msg":"Duplicate order sent."}"#),
            Some(BINANCE_DUPLICATE_ORDER)
        );
        assert_eq!(
            binance_error_code(r#"{"code":-2013,"msg":"Order does not exist."}"#),
            Some(BINANCE_ORDER_DOES_NOT_EXIST)
        );
        assert_eq!(binance_error_code("<html>502</html>"), None);
    }

    #[test]
    fn test_duplicate_order_rejection_requires_duplicate_message() {
        assert!(is_duplicate_order_rejection(
            r#"{"code":-2010,"msg":"Duplicate order sent."}"#
        ));
        // Same code, different cause: a definitive rejection, not an ambiguous one.
        assert!(!is_duplicate_order_rejection(
            r#"{"code":-2010,"msg":"Account has insufficient balance for requested action."}"#
        ));
        assert!(!is_duplicate_order_rejection(
            r#"{"code":-1013,"msg":"Duplicate order sent."}"#
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ignored by default: BinanceExecutionService::new() triggers macOS system-configuration
    /// (reqwest/native-tls) which panics in sandbox or headless CI. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn test_hmac_signature_format() {
        let service = BinanceExecutionService::new(
            "test_key".to_string(),
            "test_secret".to_string(),
            "https://api.binance.com".to_string(),
        );

        let signature = service.sign_request(
            "symbol=BTCUSDT&side=BUY&type=MARKET&quantity=0.001&timestamp=1234567890",
        );

        // Verify signature is 64 hex characters
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
