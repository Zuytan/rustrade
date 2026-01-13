use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::info;

use crate::domain::repositories::{
    CandleRepository, RiskStateRepository, StrategyRepository, TradeRepository,
};
use crate::infrastructure::persistence::database::Database;
use crate::infrastructure::persistence::repositories::{
    SqliteCandleRepository, SqliteOptimizationHistoryRepository, SqliteOrderRepository,
    SqlitePerformanceSnapshotRepository, SqliteReoptimizationTriggerRepository,
    SqliteRiskStateRepository, SqliteStrategyRepository,
};

pub struct PersistenceHandle {
    pub db: Database,
    pub candle_repository: Arc<dyn CandleRepository>,
    pub order_repository: Arc<dyn TradeRepository>,
    pub strategy_repository: Arc<dyn StrategyRepository>,
    pub risk_state_repository: Arc<dyn RiskStateRepository>,
    // Optimization Repositories
    pub opt_history_repo: Arc<SqliteOptimizationHistoryRepository>,
    pub snapshot_repo: Arc<SqlitePerformanceSnapshotRepository>,
    pub trigger_repo: Arc<SqliteReoptimizationTriggerRepository>,
}

pub struct PersistenceBootstrap;

impl PersistenceBootstrap {
    pub async fn init() -> Result<PersistenceHandle> {
        let db_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://rustrade.db".to_string());
        info!("Initializing Database at {}", db_url);

        let db = Database::new(&db_url)
            .await
            .context("Failed to initialize database")?;

        let candle_repo = Arc::new(SqliteCandleRepository::new(db.pool.clone()));
        let order_repo = Arc::new(SqliteOrderRepository::new(db.pool.clone()));
        let strategy_repo = Arc::new(SqliteStrategyRepository::new(db.pool.clone()));
        let risk_state_repo = Arc::new(SqliteRiskStateRepository::new(db.clone()));

        // Optimization
        let opt_history_repo = Arc::new(SqliteOptimizationHistoryRepository::new(db.pool.clone()));
        let snapshot_repo = Arc::new(SqlitePerformanceSnapshotRepository::new(db.pool.clone()));
        let trigger_repo = Arc::new(SqliteReoptimizationTriggerRepository::new(db.pool.clone()));

        Ok(PersistenceHandle {
            db,
            candle_repository: candle_repo,
            order_repository: order_repo,
            strategy_repository: strategy_repo,
            risk_state_repository: risk_state_repo,
            opt_history_repo,
            snapshot_repo,
            trigger_repo,
        })
    }
}
