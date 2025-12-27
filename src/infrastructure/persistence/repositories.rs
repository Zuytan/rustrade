use crate::domain::types::{Candle, Order, OrderSide};
use anyhow::{Context, Result};
use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use tracing::{error, info};

pub struct OrderRepository {
    pool: SqlitePool,
}

impl OrderRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save(&self, order: &Order) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO orders (id, symbol, side, price, quantity, timestamp)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(&order.id)
        .bind(&order.symbol)
        .bind(format!("{}", order.side)) // Enum as string
        .bind(order.price.to_string())
        .bind(order.quantity.to_string())
        .bind(order.timestamp)
        .execute(&self.pool)
        .await
        .context("Failed to save order")?;

        info!("Persisted Order {}", order.id);
        Ok(())
    }

    pub async fn get_all(&self) -> Result<Vec<Order>> {
        let rows = sqlx::query("SELECT * FROM orders ORDER BY timestamp DESC")
            .fetch_all(&self.pool)
            .await?;

        let mut orders = Vec::new();
        for row in rows {
            let side_str: String = row.try_get("side")?;
            let side = match side_str.as_str() {
                "BUY" => OrderSide::Buy, // Note uppercase from Display trait if used, or define explicitly
                "SELL" => OrderSide::Sell,
                "Buy" => OrderSide::Buy, // Handle potential variations
                "Sell" => OrderSide::Sell,
                _ => OrderSide::Buy,
            };

            orders.push(Order {
                id: row.try_get("id")?,
                symbol: row.try_get("symbol")?,
                side,
                price: Decimal::from_str(row.try_get("price")?).unwrap_or_default(),
                quantity: Decimal::from_str(row.try_get("quantity")?).unwrap_or_default(),
                timestamp: row.try_get("timestamp")?,
            });
        }

        Ok(orders)
    }
}

pub struct CandleRepository {
    pool: SqlitePool,
}

impl CandleRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save(&self, candle: &Candle) -> Result<()> {
        // Use UPSERT to avoid crashing on duplicates (if re-processing)
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO candles (symbol, timestamp, open, high, low, close, volume)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&candle.symbol)
        .bind(candle.timestamp)
        .bind(candle.open.to_string())
        .bind(candle.high.to_string())
        .bind(candle.low.to_string())
        .bind(candle.close.to_string())
        .bind(candle.volume as i64)
        .execute(&self.pool)
        .await
        .context("Failed to save candle")?;

        Ok(())
    }

    pub async fn get_range(
        &self,
        symbol: &str,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<Candle>> {
        let rows = sqlx::query(
            "SELECT * FROM candles WHERE symbol = ? AND timestamp >= ? AND timestamp <= ? ORDER BY timestamp ASC",
        )
        .bind(symbol)
        .bind(start_ts)
        .bind(end_ts)
        .fetch_all(&self.pool)
        .await?;

        let mut candles = Vec::new();
        for row in rows {
            candles.push(Candle {
                symbol: row.try_get("symbol")?,
                timestamp: row.try_get("timestamp")?,
                open: Decimal::from_str(row.try_get("open")?).unwrap_or_default(),
                high: Decimal::from_str(row.try_get("high")?).unwrap_or_default(),
                low: Decimal::from_str(row.try_get("low")?).unwrap_or_default(),
                close: Decimal::from_str(row.try_get("close")?).unwrap_or_default(),
                volume: row.try_get::<i64, _>("volume")? as u64,
            });
        }
        Ok(candles)
    }
    
    /// Delete candles older than X days
    pub async fn prune(&self, days_retention: i64) -> Result<u64> {
        let cutoff_ts = Utc::now().timestamp() - (days_retention * 24 * 60 * 60);
        
        let result = sqlx::query("DELETE FROM candles WHERE timestamp < ?")
            .bind(cutoff_ts)
            .execute(&self.pool)
            .await?;
            
        Ok(result.rows_affected())
    }
}
