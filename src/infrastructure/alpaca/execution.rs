use super::trading_stream::AlpacaTradingStream;
use crate::domain::ports::ExecutionService;
use crate::domain::ports::OrderUpdate;
use crate::domain::trading::types::{Order, OrderSide};
use crate::infrastructure::core::http_client_factory::{HttpClientFactory, build_url_with_query};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{error, info};

// ===== Execution Service (REST API) =====

pub struct AlpacaExecutionService {
    client: ClientWithMiddleware,
    api_key: String,
    api_secret: String,
    base_url: String,
    trading_stream: Arc<AlpacaTradingStream>,
    #[allow(dead_code)] // Used in background polling task
    circuit_breaker: Arc<crate::infrastructure::core::circuit_breaker::CircuitBreaker>,
    portfolio: Arc<RwLock<crate::domain::trading::portfolio::Portfolio>>, // Renamed from portfolio_cache and now injected
}

impl AlpacaExecutionService {
    pub fn new(
        api_key: String,
        api_secret: String,
        base_url: String,
        portfolio: Arc<RwLock<crate::domain::trading::portfolio::Portfolio>>,
    ) -> Self {
        let client = HttpClientFactory::create_client();
        let trading_stream = Arc::new(AlpacaTradingStream::new(
            api_key.clone(),
            api_secret.clone(),
            base_url.clone(),
        ));

        let circuit_breaker = Arc::new(
            crate::infrastructure::core::circuit_breaker::CircuitBreaker::new(
                "AlpacaAPI",
                5,
                2,
                std::time::Duration::from_secs(30),
            ),
        );

        // Spawn background polling task
        let portfolio_clone = portfolio.clone();
        let client_clone = client.clone();
        let breaker_clone = circuit_breaker.clone();
        let api_key_clone = api_key.clone();
        let api_secret_clone = api_secret.clone();
        let base_url_clone = base_url.clone();

        tokio::spawn(async move {
            info!("AlpacaExecutionService: Starting background portfolio poller");
            loop {
                let fetch_result = async {
                    let account_url = format!("{}/v2/account", base_url_clone);
                    let positions_url = format!("{}/v2/positions", base_url_clone);

                    let account_resp_raw = client_clone
                        .get(&account_url)
                        .header("APCA-API-KEY-ID", &api_key_clone)
                        .header("APCA-API-SECRET-KEY", &api_secret_clone)
                        .send()
                        .await
                        .context("Failed to send account request")?;

                    let account_text = account_resp_raw
                        .text()
                        .await
                        .context("Failed to read account response text")?;
                    let account_resp: AlpacaAccount =
                        serde_json::from_str(&account_text).map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to decode Alpaca Account: {}. Body: {}",
                                e,
                                account_text
                            )
                        })?;

                    let positions_resp_raw = client_clone
                        .get(&positions_url)
                        .header("APCA-API-KEY-ID", &api_key_clone)
                        .header("APCA-API-SECRET-KEY", &api_secret_clone)
                        .send()
                        .await
                        .context("Failed to send positions request")?;

                    let positions_text = positions_resp_raw
                        .text()
                        .await
                        .context("Failed to read positions response text")?;
                    let positions_resp: Vec<AlpacaPosition> = serde_json::from_str(&positions_text)
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to decode Alpaca Positions: {}. Body: {}",
                                e,
                                positions_text
                            )
                        })?;

                    let mut portfolio = crate::domain::trading::portfolio::Portfolio::new();
                    let cash = account_resp
                        .cash
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO);

                    portfolio.cash = cash;
                    portfolio.day_trades_count = account_resp.daytrade_count as u64;

                    for alp_pos in positions_resp {
                        let alp_symbol = alp_pos.symbol.clone();

                        let normalized_symbol = if alp_pos.asset_class.as_deref() == Some("crypto")
                        {
                            crate::domain::trading::types::normalize_crypto_symbol(&alp_symbol)
                                .map_err(|e| {
                                    anyhow::anyhow!(
                                        "Symbol normalization failed for {}: {}",
                                        alp_symbol,
                                        e
                                    )
                                })?
                        } else {
                            alp_symbol.clone()
                        };

                        let pos = crate::domain::trading::portfolio::Position {
                            symbol: normalized_symbol.clone(),
                            quantity: alp_pos.qty.parse::<Decimal>().unwrap_or(Decimal::ZERO),
                            average_price: alp_pos
                                .avg_entry_price
                                .parse::<Decimal>()
                                .unwrap_or(Decimal::ZERO),
                        };

                        portfolio.positions.insert(normalized_symbol, pos);
                    }

                    portfolio.synchronized = true;
                    Ok::<_, anyhow::Error>(portfolio)
                };

                // Execute via circuit breaker
                match breaker_clone.call(fetch_result).await {
                    Ok(portfolio) => {
                        let mut guard = portfolio_clone.write().await;
                        *guard = portfolio.clone();
                        // tracing::trace!(
                        //     "AlpacaExecutionService: Portfolio cache updated - Cash: {}, Positions: {}",
                        //     portfolio.cash,
                        //     portfolio.positions.len()
                        // );
                    }
                    Err(e) => {
                        error!("AlpacaExecutionService: Portfolio poll failed: {}", e);
                    }
                }

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });

        Self {
            client,
            api_key,
            api_secret,
            base_url,
            trading_stream,
            circuit_breaker,
            portfolio,
        }
    }
}

#[derive(Debug, Serialize)]
struct AlpacaOrderRequest {
    symbol: String,
    qty: String,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    time_in_force: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_price: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlpacaOrderResponse {
    id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct AlpacaAccount {
    cash: String,
    #[serde(rename = "buying_power")]
    _buying_power: String,
    #[serde(default)]
    daytrade_count: i64,
}

#[derive(Debug, Deserialize)]
struct AlpacaPosition {
    symbol: String,
    qty: String,
    avg_entry_price: String,
    #[serde(default)]
    asset_class: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlpacaOrder {
    id: String,
    symbol: String,
    side: String,
    qty: String,
    #[allow(dead_code)]
    filled_qty: Option<String>,
    filled_avg_price: Option<String>,
    created_at: String,
}

#[async_trait]
impl ExecutionService for AlpacaExecutionService {
    async fn execute(&self, order: Order) -> Result<()> {
        let side_str = match order.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };

        let is_fractional = !order.quantity.fract().is_zero();

        let (type_str, limit_price, stop_price) = match order.order_type {
            crate::domain::trading::types::OrderType::Market => ("market".to_string(), None, None),
            crate::domain::trading::types::OrderType::Limit => {
                ("limit".to_string(), Some(order.price.to_string()), None)
            }
            crate::domain::trading::types::OrderType::Stop => {
                ("stop".to_string(), None, Some(order.price.to_string()))
            }
            crate::domain::trading::types::OrderType::StopLimit => (
                "stop_limit".to_string(),
                Some(order.price.to_string()),
                Some(order.price.to_string()),
            ),
        };

        let (final_type, final_limit, final_stop) = if is_fractional && type_str != "market" {
            info!(
                "AlpacaExecution: Forcing MARKET order for fractional quantity {}",
                order.quantity
            );
            ("market".to_string(), None, None)
        } else {
            (type_str, limit_price, stop_price)
        };

        let is_crypto = order.symbol.contains('/') || order.symbol.contains("USD");
        let tif = if is_crypto {
            "gtc"
        } else if is_fractional {
            "day"
        } else {
            "gtc"
        };

        let order_request = AlpacaOrderRequest {
            symbol: order.symbol.clone(),
            qty: order.quantity.to_string(),
            side: side_str.to_string(),
            order_type: final_type,
            time_in_force: tif.to_string(),
            limit_price: final_limit,
            stop_price: final_stop,
        };

        let url = format!("{}/v2/orders", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .header("Content-Type", "application/json")
            .body(
                serde_json::to_string(&order_request)
                    .context("Failed to serialize order request")?,
            )
            .send()
            .await
            .context("Failed to send order to Alpaca")?;

        if response.status().is_success() {
            let order_resp: AlpacaOrderResponse = response
                .json()
                .await
                .context("Failed to parse Alpaca order response")?;
            info!(
                "Alpaca order placed: {} (status: {})",
                order_resp.id, order_resp.status
            );
            Ok(())
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("Alpaca order failed: {}", error_text)
        }
    }

    async fn get_portfolio(&self) -> Result<crate::domain::trading::portfolio::Portfolio> {
        // Return cached portfolio instantly
        let pf = self.portfolio.read().await;
        Ok(pf.clone())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let url = format!("{}/v2/orders", self.base_url);
        let url_with_query = build_url_with_query(&url, &[("status", "open")]);

        let response = self
            .client
            .get(&url_with_query)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to fetch open orders")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca open orders fetch failed: {}", error_text);
        }

        let alpaca_orders: Vec<AlpacaOrder> = response
            .json()
            .await
            .context("Failed to parse open orders")?;

        let orders = alpaca_orders
            .into_iter()
            .map(|ao| {
                let side = match ao.side.as_str() {
                    "buy" => OrderSide::Buy,
                    "sell" => OrderSide::Sell,
                    _ => OrderSide::Buy,
                };

                let qty = Decimal::from_str(&ao.qty).unwrap_or(Decimal::ZERO);

                Order {
                    id: ao.id,
                    symbol: ao.symbol,
                    side,
                    price: Decimal::ZERO,
                    quantity: qty,
                    order_type: crate::domain::trading::types::OrderType::Market,
                    timestamp: chrono::DateTime::parse_from_rfc3339(&ao.created_at)
                        .unwrap_or_default()
                        .timestamp(),
                }
            })
            .collect();

        Ok(orders)
    }

    async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let url = format!("{}/v2/orders/{}", self.base_url, order_id);

        let response = self
            .client
            .delete(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to cancel order")?;

        if !response.status().is_success() {
            if response.status().as_u16() == 404 {
                info!(
                    "AlpacaExecution: Order {} not found for cancellation (already closed?)",
                    order_id
                );
                return Ok(());
            }
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca cancel order failed: {}", error_text);
        }

        info!(
            "AlpacaExecution: Order {} cancelled successfully.",
            order_id
        );
        Ok(())
    }

    async fn get_today_orders(&self) -> Result<Vec<Order>> {
        let url = format!("{}/v2/orders", self.base_url);
        let url_with_query = build_url_with_query(&url, &[("status", "all"), ("limit", "100")]);

        let response = self
            .client
            .get(&url_with_query)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await
            .context("Failed to fetch today orders from Alpaca")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Alpaca orders fetch failed: {}", error_text);
        }

        let alp_orders: Vec<AlpacaOrder> = response
            .json()
            .await
            .context("Failed to parse Alpaca orders")?;

        let mut orders = Vec::new();
        for ao in alp_orders {
            let side = if ao.side == "buy" {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            };
            let qty = ao.qty.parse::<Decimal>().unwrap_or(Decimal::ZERO);
            let price = ao
                .filled_avg_price
                .as_ref()
                .and_then(|p| p.parse::<Decimal>().ok())
                .unwrap_or(Decimal::ZERO);

            let created_at = chrono::DateTime::parse_from_rfc3339(&ao.created_at)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or(0);

            orders.push(Order {
                id: ao.id,
                symbol: ao.symbol,
                side,
                price,
                quantity: qty,
                order_type: crate::domain::trading::types::OrderType::Market,
                timestamp: created_at,
            });
        }

        Ok(orders)
    }

    async fn subscribe_order_updates(&self) -> Result<broadcast::Receiver<OrderUpdate>> {
        Ok(self.trading_stream.subscribe())
    }
}
