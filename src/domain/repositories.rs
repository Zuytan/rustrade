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

use crate::domain::portfolio::Portfolio;
use crate::domain::types::Order;
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
