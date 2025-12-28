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
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)
                        .await
                        .context("Failed to create database directory")?;
                }
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

        info!("Database schema initialized.");
        Ok(())
    }
}
