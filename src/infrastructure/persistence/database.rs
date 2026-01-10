use anyhow::{Context, Result};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;
use tokio::fs;
use tracing::info;

/// Singleton database wrapper
#[allow(dead_code)]
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
        db.init().await?;

        Ok(db)
    }

    /// Initialize database schema
    async fn init(&self) -> Result<()> {
        let mut conn = self.pool.acquire().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orders (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                price TEXT NOT NULL,
                quantity TEXT NOT NULL,
                order_type TEXT DEFAULT 'MARKET',
                timestamp INTEGER NOT NULL
            );
            "#,
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create orders table")?;

        // Migration: Attempt to add order_type column if it doesn't exist (for existing DBs)
        // We ignore error if column already exists (Generic error handling for now)
        let _ = sqlx::query("ALTER TABLE orders ADD COLUMN order_type TEXT DEFAULT 'MARKET'")
            .execute(&mut *conn)
            .await;

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

        info!("Database schema initialized.");
        Ok(())
    }
}
