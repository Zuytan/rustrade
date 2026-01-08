use crate::domain::risk::state::RiskState;
use crate::domain::repositories::RiskStateRepository;
use crate::infrastructure::persistence::database::Database;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::str::FromStr;

pub struct SqliteRiskStateRepository {
    database: Database,
}

impl SqliteRiskStateRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }
}

#[async_trait]
impl RiskStateRepository for SqliteRiskStateRepository {
    /// Save the risk state to the database (upsert)
    async fn save(&self, state: &RiskState) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO risk_state (
                id, 
                session_start_equity, 
                daily_start_equity, 
                equity_high_water_mark, 
                consecutive_losses, 
                reference_date, 
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP)
            ON CONFLICT(id) DO UPDATE SET
                session_start_equity = excluded.session_start_equity,
                daily_start_equity = excluded.daily_start_equity,
                equity_high_water_mark = excluded.equity_high_water_mark,
                consecutive_losses = excluded.consecutive_losses,
                reference_date = excluded.reference_date,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(&state.id)
        .bind(state.session_start_equity.to_string())
        .bind(state.daily_start_equity.to_string())
        .bind(state.equity_high_water_mark.to_string())
        .bind(state.consecutive_losses as i64)
        .bind(state.reference_date)
        .execute(&self.database.pool)
        .await
        .context("Failed to save risk state")?;

        Ok(())
    }

    /// Load the risk state from the database
    async fn load(&self, id: &str) -> Result<Option<RiskState>> {
        let row = sqlx::query_as::<_, (String, String, String, String, i64, NaiveDate)>(
            r#"
            SELECT 
                id, 
                session_start_equity, 
                daily_start_equity, 
                equity_high_water_mark, 
                consecutive_losses, 
                reference_date
            FROM risk_state
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.database.pool)
        .await
        .context("Failed to load risk state")?;

        if let Some((id, session_eq_str, daily_eq_str, hwm_eq_str, losses, ref_date)) = row {
            Ok(Some(RiskState {
                id,
                session_start_equity: Decimal::from_str(&session_eq_str).unwrap_or_default(),
                daily_start_equity: Decimal::from_str(&daily_eq_str).unwrap_or_default(),
                equity_high_water_mark: Decimal::from_str(&hwm_eq_str).unwrap_or_default(),
                consecutive_losses: losses as usize,
                reference_date: ref_date,
                updated_at: chrono::Utc::now().timestamp(),
                daily_drawdown_reset: false,
            }))
        } else {
            Ok(None)
        }
    }
}
