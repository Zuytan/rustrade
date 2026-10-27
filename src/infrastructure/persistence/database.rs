use anyhow::{Context, Result};

use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;
use tokio::fs;
use tracing::info;

/// Latest schema version, stored in SQLite's `PRAGMA user_version`.
///
/// To change the schema: add an `apply_vN` step, call it from `migrate`, and bump this
/// constant. Never edit a released step: existing databases have already applied it.
pub const SCHEMA_VERSION: i64 = 1;

/// Singleton database wrapper

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(db_url: &str) -> Result<Self> {
        // Ensure the directory exists if it's a file path
        if let Some(path_part) = db_url.strip_prefix("sqlite://") {
            let path = Path::new(path_part);
            if let Some(parent) = path.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)
                    .await
                    .context("Failed to create database directory")?;
            }
        }

        let options = SqliteConnectOptions::from_str(db_url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal); // Better for concurrency

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to SQLite database")?;

        info!("Connected to database: {}", db_url);

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    /// Brings the schema to `SCHEMA_VERSION`, one versioned step at a time.
    ///
    /// Each step runs in a transaction with the version bump, so a failure leaves the
    /// database at its previous version and stops startup. A database written by a newer
    /// build is refused instead of being used with an unknown schema.
    async fn migrate(&self) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to start schema migration")?;

        let current: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&mut *tx)
            .await
            .context("Failed to read schema version")?;

        if current > SCHEMA_VERSION {
            anyhow::bail!(
                "Database schema version {} is newer than this build supports ({}). \
                 Upgrade Rustrade or restore a compatible database backup.",
                current,
                SCHEMA_VERSION
            );
        }

        if current < 1 {
            Self::apply_v1(&mut tx).await?;
        }

        if current < SCHEMA_VERSION {
            // PRAGMA does not accept bound parameters; the value is an internal constant.
            sqlx::query(&format!("PRAGMA user_version = {}", SCHEMA_VERSION))
                .execute(&mut *tx)
                .await
                .context("Failed to record schema version")?;
        }

        tx.commit()
            .await
            .context("Failed to commit schema migration")?;

        if current < SCHEMA_VERSION {
            info!(
                "Database schema migrated from version {} to {}.",
                current, SCHEMA_VERSION
            );
        } else {
            info!("Database schema up to date (version {}).", SCHEMA_VERSION);
        }
        Ok(())
    }

    /// Adds a column unless it already exists (SQLite has no `ADD COLUMN IF NOT EXISTS`).
    async fn ensure_column(
        conn: &mut SqliteConnection,
        table: &str,
        column: &str,
        definition: &str,
    ) -> Result<()> {
        let existing: Vec<String> =
            sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{}')", table))
                .fetch_all(&mut *conn)
                .await
                .with_context(|| format!("Failed to inspect table {}", table))?;

        if !existing.iter().any(|c| c == column) {
            sqlx::query(&format!(
                "ALTER TABLE {} ADD COLUMN {} {}",
                table, column, definition
            ))
            .execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to add column {}.{}", table, column))?;
            info!("Database: added column {}.{}", table, column);
        }
        Ok(())
    }

    /// Version 1: baseline schema. Idempotent, so databases created before versioning
    /// (user_version 0, possibly missing later columns) are upgraded in place.
    async fn apply_v1(conn: &mut SqliteConnection) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orders (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                price TEXT NOT NULL,
                quantity TEXT NOT NULL,
                order_type TEXT DEFAULT 'MARKET',
                status TEXT DEFAULT 'NEW',
                timestamp INTEGER NOT NULL,
                correlation_id TEXT
            );
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create orders table")?;

        // Columns added after the first releases
        Self::ensure_column(conn, "orders", "order_type", "TEXT DEFAULT 'MARKET'").await?;
        Self::ensure_column(conn, "orders", "status", "TEXT DEFAULT 'NEW'").await?;
        Self::ensure_column(conn, "orders", "correlation_id", "TEXT").await?;

        // 2. Candles Table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS candles (
                symbol TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                open TEXT NOT NULL,
                high TEXT NOT NULL,
                low TEXT NOT NULL,
                close TEXT NOT NULL,
                volume INTEGER NOT NULL,
                PRIMARY KEY (symbol, timestamp)
            );
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create candles table")?;

        // Index for faster time-range queries on candles
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_candles_symbol_time 
            ON candles (symbol, timestamp);
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create candle index")?;

        // 3. Symbol Strategies Table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS symbol_strategies (
                symbol TEXT PRIMARY KEY,
                strategy_mode TEXT NOT NULL,
                config_json TEXT NOT NULL,
                is_active BOOLEAN DEFAULT 1,
                last_updated INTEGER
            );
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create symbol_strategies table")?;

        // 4. Optimization History Table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS optimization_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                parameters_json TEXT NOT NULL,
                performance_metrics_json TEXT NOT NULL,
                market_regime TEXT,
                sharpe_ratio REAL,
                total_return REAL,
                win_rate REAL,
                is_active BOOLEAN DEFAULT 0,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_opt_history_symbol_active 
            ON optimization_history (symbol, is_active);
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create optimization_history table")?;

        // 5. Performance Snapshots Table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS performance_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                equity REAL NOT NULL,
                drawdown_pct REAL,
                sharpe_rolling_30d REAL,
                win_rate_rolling_30d REAL,
                regime TEXT,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_perf_snapshot_symbol_time 
            ON performance_snapshots (symbol, timestamp);
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create performance_snapshots table")?;

        // 6. Reoptimization Triggers Table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS reoptimization_triggers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                trigger_reason TEXT NOT NULL,
                status TEXT DEFAULT 'pending',
                result_json TEXT,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_reopt_trigger_status 
            ON reoptimization_triggers (status, timestamp);
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create reoptimization_triggers table")?;

        // 7. Risk State Table (Global Singleton)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS risk_state (
                id TEXT PRIMARY KEY,
                session_start_equity TEXT NOT NULL,
                daily_start_equity TEXT NOT NULL,
                equity_high_water_mark TEXT NOT NULL,
                consecutive_losses INTEGER NOT NULL,
                reference_date DATE NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create risk_state table")?;

        // 8. Completed Trades (enriched for post-mortem analysis)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS completed_trades (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                entry_price TEXT NOT NULL,
                exit_price TEXT,
                quantity TEXT NOT NULL,
                pnl TEXT NOT NULL,
                entry_timestamp INTEGER NOT NULL,
                exit_timestamp INTEGER,
                strategy_used TEXT,
                regime_detected TEXT,
                entry_reason TEXT,
                exit_reason TEXT,
                slippage TEXT,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_completed_trades_symbol ON completed_trades (symbol);
            CREATE INDEX IF NOT EXISTS idx_completed_trades_entry_ts ON completed_trades (entry_timestamp);
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create completed_trades table")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// File-backed temporary database (each `:memory:` connection would be a separate DB).
    struct TempDb {
        path: std::path::PathBuf,
    }

    impl TempDb {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rustrade-migration-test-{}.db",
                uuid::Uuid::new_v4()
            ));
            Self { path }
        }

        fn url(&self) -> String {
            format!("sqlite://{}", self.path.display())
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{}", self.path.display(), suffix));
            }
        }
    }

    async fn user_version(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(pool)
            .await
            .expect("user_version")
    }

    async fn columns(pool: &SqlitePool, table: &str) -> Vec<String> {
        sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{}')", table))
            .fetch_all(pool)
            .await
            .expect("table_info")
    }

    async fn raw_pool(url: &str) -> SqlitePool {
        let options = SqliteConnectOptions::from_str(url)
            .expect("url")
            .create_if_missing(true);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("raw pool")
    }

    #[tokio::test]
    async fn test_fresh_database_is_migrated_to_latest() {
        let tmp = TempDb::new();
        let db = Database::new(&tmp.url()).await.expect("open");

        assert_eq!(user_version(&db.pool).await, SCHEMA_VERSION);
        let order_cols = columns(&db.pool, "orders").await;
        for col in ["order_type", "status", "correlation_id"] {
            assert!(order_cols.contains(&col.to_string()), "missing {}", col);
        }
        assert!(!columns(&db.pool, "risk_state").await.is_empty());
    }

    #[tokio::test]
    async fn test_legacy_database_gets_missing_columns() {
        let tmp = TempDb::new();
        {
            // Schema of an old release: orders without the later columns, no version.
            let pool = raw_pool(&tmp.url()).await;
            sqlx::query(
                "CREATE TABLE orders (id TEXT PRIMARY KEY, symbol TEXT NOT NULL, side TEXT NOT NULL, \
                 price TEXT NOT NULL, quantity TEXT NOT NULL, timestamp INTEGER NOT NULL)",
            )
            .execute(&pool)
            .await
            .expect("legacy table");
            sqlx::query(
                "INSERT INTO orders (id, symbol, side, price, quantity, timestamp) \
                 VALUES ('o1', 'AAPL', 'Buy', '100', '1', 0)",
            )
            .execute(&pool)
            .await
            .expect("legacy row");
            pool.close().await;
        }

        let db = Database::new(&tmp.url()).await.expect("migrate legacy");
        assert_eq!(user_version(&db.pool).await, SCHEMA_VERSION);
        let order_cols = columns(&db.pool, "orders").await;
        for col in ["order_type", "status", "correlation_id"] {
            assert!(order_cols.contains(&col.to_string()), "missing {}", col);
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
            .fetch_one(&db.pool)
            .await
            .expect("count");
        assert_eq!(count, 1, "existing data must be preserved");
    }

    #[tokio::test]
    async fn test_reopening_is_idempotent() {
        let tmp = TempDb::new();
        let db = Database::new(&tmp.url()).await.expect("first open");
        db.pool.close().await;
        let db = Database::new(&tmp.url()).await.expect("second open");
        assert_eq!(user_version(&db.pool).await, SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn test_newer_schema_is_refused() {
        let tmp = TempDb::new();
        {
            let pool = raw_pool(&tmp.url()).await;
            sqlx::query(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1))
                .execute(&pool)
                .await
                .expect("set version");
            pool.close().await;
        }

        let err = match Database::new(&tmp.url()).await {
            Ok(_) => panic!("a newer schema must be refused"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("newer"));
    }
}
