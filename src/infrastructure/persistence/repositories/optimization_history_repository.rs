use crate::domain::market::market_regime::MarketRegimeType;
use crate::domain::optimization::optimization_history::OptimizationHistory;
use crate::domain::repositories::OptimizationHistoryRepository;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use sqlx::{Row, SqlitePool};

pub struct SqliteOptimizationHistoryRepository {
    pool: SqlitePool,
}

impl SqliteOptimizationHistoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OptimizationHistoryRepository for SqliteOptimizationHistoryRepository {
    async fn save(&self, history: &OptimizationHistory) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO optimization_history 
            (symbol, timestamp, parameters_json, performance_metrics_json, market_regime, sharpe_ratio, total_return, win_rate, is_active)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&history.symbol)
        .bind(history.timestamp.timestamp())
        .bind(&history.parameters_json)
        .bind(&history.performance_metrics_json)
        .bind(history.market_regime.to_string()) // Assuming Display impl is compatible or convert manually
        .bind(history.sharpe_ratio)
        .bind(history.total_return)
        .bind(history.win_rate)
        .bind(history.is_active)
        .execute(&self.pool)
        .await
        .context("Failed to save optimization history")?;

        Ok(())
    }

    async fn get_latest_active(&self, symbol: &str) -> Result<Option<OptimizationHistory>> {
        let row = sqlx::query(
            "SELECT * FROM optimization_history WHERE symbol = ? AND is_active = 1 ORDER BY timestamp DESC LIMIT 1"
        )
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let regime_str: String = row.try_get("market_regime")?;
            let market_regime = match regime_str.as_str() {
                "Trending Up" => MarketRegimeType::TrendingUp,
                "Trending Down" => MarketRegimeType::TrendingDown,
                "Ranging" => MarketRegimeType::Ranging,
                "Volatile" => MarketRegimeType::Volatile,
                _ => MarketRegimeType::Unknown,
            };

            Ok(Some(OptimizationHistory {
                id: Some(row.try_get("id")?),
                symbol: row.try_get("symbol")?,
                timestamp: Utc.timestamp_opt(row.try_get("timestamp")?, 0).unwrap(),
                parameters_json: row.try_get("parameters_json")?,
                performance_metrics_json: row.try_get("performance_metrics_json")?,
                market_regime,
                sharpe_ratio: row.try_get("sharpe_ratio")?,
                total_return: row.try_get("total_return")?,
                win_rate: row.try_get("win_rate")?,
                is_active: row.try_get("is_active")?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn find_by_symbol(&self, symbol: &str, limit: usize) -> Result<Vec<OptimizationHistory>> {
        let rows = sqlx::query(
            "SELECT * FROM optimization_history WHERE symbol = ? ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(symbol)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut history_list = Vec::new();
        for row in rows {
            let regime_str: String = row.try_get("market_regime")?;
            let market_regime = match regime_str.as_str() {
                "Trending Up" => MarketRegimeType::TrendingUp,
                "Trending Down" => MarketRegimeType::TrendingDown,
                "Ranging" => MarketRegimeType::Ranging,
                "Volatile" => MarketRegimeType::Volatile,
                _ => MarketRegimeType::Unknown,
            };

            history_list.push(OptimizationHistory {
                id: Some(row.try_get("id")?),
                symbol: row.try_get("symbol")?,
                timestamp: Utc.timestamp_opt(row.try_get("timestamp")?, 0).unwrap(),
                parameters_json: row.try_get("parameters_json")?,
                performance_metrics_json: row.try_get("performance_metrics_json")?,
                market_regime,
                sharpe_ratio: row.try_get("sharpe_ratio")?,
                total_return: row.try_get("total_return")?,
                win_rate: row.try_get("win_rate")?,
                is_active: row.try_get("is_active")?,
            });
        }
        Ok(history_list)
    }

    async fn deactivate_old(&self, symbol: &str) -> Result<()> {
        sqlx::query("UPDATE optimization_history SET is_active = 0 WHERE symbol = ?")
            .bind(symbol)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
