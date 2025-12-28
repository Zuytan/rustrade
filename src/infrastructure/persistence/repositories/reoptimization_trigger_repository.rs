use crate::domain::optimization::reoptimization_trigger::{ReoptimizationTrigger, TriggerReason};
use crate::domain::repositories::ReoptimizationTriggerRepository;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use sqlx::{Row, SqlitePool};

pub struct SqliteReoptimizationTriggerRepository {
    pool: SqlitePool,
}

impl SqliteReoptimizationTriggerRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReoptimizationTriggerRepository for SqliteReoptimizationTriggerRepository {
    async fn save(&self, trigger: &ReoptimizationTrigger) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO reoptimization_triggers 
            (symbol, timestamp, trigger_reason, status, result_json)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&trigger.symbol)
        .bind(trigger.timestamp.timestamp())
        .bind(trigger.trigger_reason.to_string())
        .bind(&trigger.status)
        .bind(&trigger.result_json)
        .execute(&self.pool)
        .await
        .context("Failed to save reoptimization trigger")?;

        Ok(())
    }

    async fn get_pending(&self) -> Result<Vec<ReoptimizationTrigger>> {
        let rows = sqlx::query(
            "SELECT * FROM reoptimization_triggers WHERE status = 'pending' ORDER BY timestamp ASC"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut triggers = Vec::new();
        for row in rows {
            let reason_str: String = row.try_get("trigger_reason")?;
            let trigger_reason = match reason_str.as_str() {
                "Poor Performance" => TriggerReason::PoorPerformance,
                "Regime Change" => TriggerReason::RegimeChange,
                "Drawdown Limit" => TriggerReason::DrawdownLimit,
                "Scheduled" => TriggerReason::Scheduled,
                "Manual" => TriggerReason::Manual,
                _ => TriggerReason::Scheduled, // Fallback
            };

            triggers.push(ReoptimizationTrigger {
                id: Some(row.try_get("id")?),
                symbol: row.try_get("symbol")?,
                timestamp: Utc.timestamp_opt(row.try_get("timestamp")?, 0).unwrap(),
                trigger_reason,
                status: row.try_get("status")?,
                result_json: row.try_get("result_json")?,
            });
        }
        Ok(triggers)
    }

    async fn update_status(&self, id: i64, status: &str, result: Option<String>) -> Result<()> {
        if let Some(res) = result {
             sqlx::query("UPDATE reoptimization_triggers SET status = ?, result_json = ? WHERE id = ?")
                .bind(status)
                .bind(res)
                .bind(id)
                .execute(&self.pool)
                .await?;
        } else {
             sqlx::query("UPDATE reoptimization_triggers SET status = ? WHERE id = ?")
                .bind(status)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
}
