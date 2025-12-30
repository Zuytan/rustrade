use crate::domain::market::market_regime::MarketRegimeType;
use crate::domain::performance::performance_snapshot::PerformanceSnapshot;
use crate::domain::repositories::PerformanceSnapshotRepository;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use sqlx::{Row, SqlitePool};

pub struct SqlitePerformanceSnapshotRepository {
    pool: SqlitePool,
}

impl SqlitePerformanceSnapshotRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PerformanceSnapshotRepository for SqlitePerformanceSnapshotRepository {
    async fn save(&self, snapshot: &PerformanceSnapshot) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO performance_snapshots 
            (symbol, timestamp, equity, drawdown_pct, sharpe_rolling_30d, win_rate_rolling_30d, regime)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&snapshot.symbol)
        .bind(snapshot.timestamp.timestamp())
        .bind(snapshot.equity.to_f64().unwrap_or(0.0)) // Storing Decimal as REAL/f64 for simplicity in stats
        .bind(snapshot.drawdown_pct)
        .bind(snapshot.sharpe_rolling_30d)
        .bind(snapshot.win_rate_rolling_30d)
        .bind(snapshot.regime.to_string())
        .execute(&self.pool)
        .await
        .context("Failed to save performance snapshot")?;

        Ok(())
    }

    async fn get_latest(&self, symbol: &str) -> Result<Option<PerformanceSnapshot>> {
        let row = sqlx::query(
            "SELECT * FROM performance_snapshots WHERE symbol = ? ORDER BY timestamp DESC LIMIT 1",
        )
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let regime_str: String = row.try_get("regime")?;
            let market_regime = match regime_str.as_str() {
                "Trending Up" => MarketRegimeType::TrendingUp,
                "Trending Down" => MarketRegimeType::TrendingDown,
                "Ranging" => MarketRegimeType::Ranging,
                "Volatile" => MarketRegimeType::Volatile,
                _ => MarketRegimeType::Unknown,
            };

            let equity_f64: f64 = row.try_get("equity")?;

            Ok(Some(PerformanceSnapshot {
                id: Some(row.try_get("id")?),
                symbol: row.try_get("symbol")?,
                timestamp: Utc.timestamp_opt(row.try_get("timestamp")?, 0).unwrap(),
                equity: Decimal::from_f64(equity_f64).unwrap_or_default(),
                drawdown_pct: row.try_get("drawdown_pct")?,
                sharpe_rolling_30d: row.try_get("sharpe_rolling_30d")?,
                win_rate_rolling_30d: row.try_get("win_rate_rolling_30d")?,
                regime: market_regime,
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_history(&self, symbol: &str, limit: usize) -> Result<Vec<PerformanceSnapshot>> {
        let rows = sqlx::query(
            "SELECT * FROM performance_snapshots WHERE symbol = ? ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(symbol)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut snapshots = Vec::new();
        for row in rows {
            let regime_str: String = row.try_get("regime")?;
            let market_regime = match regime_str.as_str() {
                "Trending Up" => MarketRegimeType::TrendingUp,
                "Trending Down" => MarketRegimeType::TrendingDown,
                "Ranging" => MarketRegimeType::Ranging,
                "Volatile" => MarketRegimeType::Volatile,
                _ => MarketRegimeType::Unknown,
            };

            let equity_f64: f64 = row.try_get("equity")?;

            snapshots.push(PerformanceSnapshot {
                id: Some(row.try_get("id")?),
                symbol: row.try_get("symbol")?,
                timestamp: Utc.timestamp_opt(row.try_get("timestamp")?, 0).unwrap(),
                equity: Decimal::from_f64(equity_f64).unwrap_or_default(),
                drawdown_pct: row.try_get("drawdown_pct")?,
                sharpe_rolling_30d: row.try_get("sharpe_rolling_30d")?,
                win_rate_rolling_30d: row.try_get("win_rate_rolling_30d")?,
                regime: market_regime,
            });
        }
        Ok(snapshots)
    }
}
