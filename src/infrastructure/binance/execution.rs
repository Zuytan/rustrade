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
    Order, OrderSide, OrderType, denormalize_crypto_symbol, normalize_crypto_symbol,
};
use crate::infrastructure::core::circuit_breaker::CircuitBreaker;
use crate::infrastructure::core::http_client_factory::HttpClientFactory;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::TimeZone;
use hmac::{Hmac, Mac};
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

pub struct BinanceExecutionService {
    client: ClientWithMiddleware,
    api_key: String,
    api_secret: String,
    base_url: String,
    order_update_tx: broadcast::Sender<OrderUpdate>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl BinanceExecutionService {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        let client = HttpClientFactory::create_client();
        let (order_update_tx, _) = broadcast::channel(100);
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            "BinanceExecution",
            5,
            3,
            std::time::Duration::from_secs(60),
        ));

        Self {
            client,
            api_key,
            api_secret,
            base_url,
            order_update_tx,
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
    async fn execute(&self, order: Order) -> Result<()> {
        self.circuit_breaker
            .call(async move {
                let api_symbol = denormalize_crypto_symbol(&order.symbol);

                let side = match order.side {
                    OrderSide::Buy => "BUY",
                    OrderSide::Sell => "SELL",
                };

                let order_type = match order.order_type {
                    OrderType::Market => "MARKET",
                    OrderType::Limit => "LIMIT",
                    OrderType::Stop => "STOP_LOSS",
                    OrderType::StopLimit => "STOP_LOSS_LIMIT",
                };

                let timestamp = chrono::Utc::now().timestamp_millis();

                let mut params = vec![
                    ("symbol", api_symbol.clone()),
                    ("side", side.to_string()),
                    ("type", order_type.to_string()),
                    ("quantity", order.quantity.to_string()),
                    ("newClientOrderId", order.id.clone()),
                    ("timestamp", timestamp.to_string()),
                ];

                if let OrderType::Limit = order.order_type
                    && order.price > Decimal::ZERO
                {
                    params.push(("price", order.price.to_string()));
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

                let response = self
                    .client
                    .post(&url)
                    .header("X-MBX-APIKEY", &self.api_key)
                    .send()
                    .await
                    .context("Failed to place order on Binance")?;

                if !response.status().is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    anyhow::bail!("Binance order placement failed: {}", error_text);
                }

                let response_json: serde_json::Value = response.json().await?;
                info!("Binance order placed successfully: {:?}", response_json);

                Ok(())
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
        // Get orders from start of today (UTC)
        let today_start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("midnight is always a valid time");
        let _start_time = chrono::Utc
            .from_utc_datetime(&today_start)
            .timestamp_millis();

        // Note: Binance requires symbol for allOrders endpoint
        // For simplicity, return empty for now (can be enhanced to query all symbols)
        warn!(
            "BinanceExecutionService::get_today_orders not fully implemented - requires symbol parameter"
        );
        Ok(vec![])
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
                })
            })
            .collect();

        Ok(orders)
    }

    async fn cancel_order(&self, _order_id: &str) -> Result<()> {
        // Note: Binance requires symbol for order cancellation
        // For now, return error (can be enhanced to store symbol mapping)
        anyhow::bail!("BinanceExecutionService::cancel_order requires symbol - not implemented");
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        // Known limitation: User Data Stream is not implemented; order status updates rely on polling.
        // Priority: Medium/Low - Current strategy relies on polling/REST. Stream needed only for HFT or high-concurrency needs.
        // This requires:
        // 1. POST /api/v3/userDataStream to get listenKey
        // 2. Connect WebSocket to wss://stream.binance.com:9443/ws/<listenKey>
        // 3. Keep listenKey alive with PUT requests every 30 minutes
        // 4. Parse executionReport events and broadcast as OrderUpdate

        Ok(self.order_update_tx.subscribe())
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
