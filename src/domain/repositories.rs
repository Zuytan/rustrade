//! Repository Pattern Abstractions
//!
//! This module defines repository traits for data persistence,
//! enabling clean separation between business logic and storage implementation.
//!
//! # Design
//!
//! Following the Repository Pattern, we define two main abstractions:
//! - `TradeRepository`: Persists and retrieves trade orders
//! - `PortfolioRepository`: Manages portfolio state and equity history
//!
//! # Current Implementation
//!
//! The `InMemory` implementations provide thread-safe, in-memory storage
//! using `Arc<RwLock>` for concurrent access.
//!
//! # Future
//!
//! These traits are designed to support PostgreSQL implementations
//! for production persistence without changing business logic.
//!
//! # Example
//!
//! ```rust,no_run
//! use rustrade::domain::repositories::TradeRepository;
//! use rustrade::infrastructure::InMemoryTradeRepository;
//!
//! # async {
//! let repo = InMemoryTradeRepository::new();
//! // repo.save(&order).await?;
//! // let trades = repo.find_by_symbol("AAPL").await?;
//! # };
//! ```

use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::Order;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// Repository for persisting and retrieving trade orders
#[async_trait]
pub trait TradeRepository: Send + Sync {
    /// Save a trade order
    async fn save(&self, trade: &Order) -> Result<()>;

    /// Find all trades for a specific symbol
    async fn find_by_symbol(&self, symbol: &str) -> Result<Vec<Order>>;

    /// Find the most recent trades
    async fn find_recent(&self, limit: usize) -> Result<Vec<Order>>;

    /// Get all trades
    async fn get_all(&self) -> Result<Vec<Order>>;

    /// Count total number of trades
    async fn count(&self) -> Result<usize>;
}

/// Repository for persisting and retrieving portfolio state
#[async_trait]
pub trait PortfolioRepository: Send + Sync {
    /// Load the current portfolio state
    async fn load(&self) -> Result<Portfolio>;

    /// Save the portfolio state
    async fn save(&self, portfolio: &Portfolio) -> Result<()>;

    /// Get equity history since a given date
    async fn get_equity_history(
        &self,
        start: DateTime<Utc>,
    ) -> Result<Vec<(DateTime<Utc>, Decimal)>>;
}

use crate::domain::trading::types::Candle;

/// Repository for persisting and retrieving market data (candles)
#[async_trait]
pub trait CandleRepository: Send + Sync {
    /// Save a candle
    async fn save(&self, candle: &Candle) -> Result<()>;

    /// Get candles for a symbol within a time range
    async fn get_range(&self, symbol: &str, start_ts: i64, end_ts: i64) -> Result<Vec<Candle>>;

    /// Get the timestamp of the most recent candle for a symbol
    /// Returns None if no candles exist for this symbol
    async fn get_latest_timestamp(&self, symbol: &str) -> Result<Option<i64>>;

    /// Count how many candles exist for a symbol within a time range
    /// Useful for determining if we have sufficient cached data
    async fn count_candles(&self, symbol: &str, start_ts: i64, end_ts: i64) -> Result<usize>;

    /// Prune old candles
    async fn prune(&self, days_retention: i64) -> Result<u64>;
}

use crate::domain::market::strategy_config::StrategyDefinition;

/// Repository for persisting and retrieving strategy configurations
#[async_trait]
pub trait StrategyRepository: Send + Sync {
    /// Save a strategy configuration
    async fn save(&self, config: &StrategyDefinition) -> Result<()>;

    /// Get strategy configuration for a symbol
    async fn find_by_symbol(&self, symbol: &str) -> Result<Option<StrategyDefinition>>;

    /// Get all active strategies
    async fn get_all_active(&self) -> Result<Vec<StrategyDefinition>>;
}

use crate::domain::optimization::optimization_history::OptimizationHistory;
use crate::domain::optimization::reoptimization_trigger::ReoptimizationTrigger;
use crate::domain::performance::performance_snapshot::PerformanceSnapshot;

/// Repository for optimization history
#[async_trait]
pub trait OptimizationHistoryRepository: Send + Sync {
    async fn save(&self, history: &OptimizationHistory) -> Result<()>;
    async fn get_latest_active(&self, symbol: &str) -> Result<Option<OptimizationHistory>>;
    async fn find_by_symbol(&self, symbol: &str, limit: usize) -> Result<Vec<OptimizationHistory>>;
    async fn deactivate_old(&self, symbol: &str) -> Result<()>;
}

/// Repository for performance snapshots
#[async_trait]
pub trait PerformanceSnapshotRepository: Send + Sync {
    async fn save(&self, snapshot: &PerformanceSnapshot) -> Result<()>;
    async fn get_latest(&self, symbol: &str) -> Result<Option<PerformanceSnapshot>>;
    async fn get_history(&self, symbol: &str, limit: usize) -> Result<Vec<PerformanceSnapshot>>;
}

/// Repository for re-optimization triggers
#[async_trait]
pub trait ReoptimizationTriggerRepository: Send + Sync {
    async fn save(&self, trigger: &ReoptimizationTrigger) -> Result<()>;
    async fn get_pending(&self) -> Result<Vec<ReoptimizationTrigger>>;
    async fn update_status(&self, id: i64, status: &str, result: Option<String>) -> Result<()>;
}
