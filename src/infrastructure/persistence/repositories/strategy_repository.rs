use crate::domain::repositories::StrategyRepository;
use crate::domain::strategy_config::{StrategyDefinition, StrategyMode};
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use tracing::info;

pub struct SqliteStrategyRepository {
    pool: SqlitePool,
}

impl SqliteStrategyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl StrategyRepository for SqliteStrategyRepository {
    async fn save(&self, config: &StrategyDefinition) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO symbol_strategies (symbol, strategy_mode, config_json, is_active, last_updated)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(symbol) DO UPDATE SET
                strategy_mode = excluded.strategy_mode,
                config_json = excluded.config_json,
                is_active = excluded.is_active,
                last_updated = excluded.last_updated
            "#,
        )
        .bind(&config.symbol)
        .bind(config.mode.to_string())
        .bind(&config.config_json)
        .bind(config.is_active)
        .bind(chrono::Utc::now().timestamp())
        .execute(&self.pool)
        .await
        .context("Failed to save strategy config")?;

        info!("Persisted Strategy Config for {}", config.symbol);
        Ok(())
    }

    async fn find_by_symbol(&self, symbol: &str) -> Result<Option<StrategyDefinition>> {
        let row = sqlx::query("SELECT * FROM symbol_strategies WHERE symbol = ?")
            .bind(symbol)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let mode_str: String = row.try_get("strategy_mode")?;
            let mode = StrategyMode::from_str(&mode_str)?;

            Ok(Some(StrategyDefinition {
                symbol: row.try_get("symbol")?,
                mode,
                config_json: row.try_get("config_json")?,
                is_active: row.try_get("is_active")?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_all_active(&self) -> Result<Vec<StrategyDefinition>> {
        let rows = sqlx::query("SELECT * FROM symbol_strategies WHERE is_active = 1")
            .fetch_all(&self.pool)
            .await?;

        let mut configs = Vec::new();
        for row in rows {
            let mode_str: String = row.try_get("strategy_mode")?;
            let mode = StrategyMode::from_str(&mode_str)?;

            configs.push(StrategyDefinition {
                symbol: row.try_get("symbol")?,
                mode,
                config_json: row.try_get("config_json")?,
                is_active: row.try_get("is_active")?,
            });
        }
        Ok(configs)
    }
}
