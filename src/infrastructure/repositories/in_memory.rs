//! In-Memory Repository Implementations
//!
//! This module provides thread-safe, in-memory implementations of the
//! repository traits defined in `domain::repositories`.
//!
//! # Features
//!
//! - **Thread-safe**: Uses `Arc<RwLock>` for concurrent access
//! - **Async**: All operations are async-ready
//! - **Testing**: Ideal for unit tests and development
//! - **Production**: Suitable for single-instance deployments
//!
//! # Limitations
//!
//! - Data is lost on application restart
//! - No persistence across multiple instances
//! - Limited by available RAM
//!
//! For production persistence, implement `TradeRepository` and
//! `PortfolioRepository` with PostgreSQL or similar.

use crate::domain::portfolio::Portfolio;
use crate::domain::repositories::{PortfolioRepository, TradeRepository};
use crate::domain::types::Order;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory implementation of TradeRepository
/// Suitable for testing and single-instance deployments
pub struct InMemoryTradeRepository {
    trades: Arc<RwLock<Vec<Order>>>,
}

impl InMemoryTradeRepository {
    pub fn new() -> Self {
        Self {
            trades: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for InMemoryTradeRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TradeRepository for InMemoryTradeRepository {
    async fn save(&self, trade: &Order) -> Result<()> {
        self.trades.write().await.push(trade.clone());
        Ok(())
    }

    async fn find_by_symbol(&self, symbol: &str) -> Result<Vec<Order>> {
        let trades = self.trades.read().await;
        Ok(trades
            .iter()
            .filter(|t| t.symbol == symbol)
            .cloned()
            .collect())
    }

    async fn find_recent(&self, limit: usize) -> Result<Vec<Order>> {
        let trades = self.trades.read().await;
        Ok(trades.iter().rev().take(limit).cloned().collect())
    }

    async fn get_all(&self) -> Result<Vec<Order>> {
        Ok(self.trades.read().await.clone())
    }

    async fn count(&self) -> Result<usize> {
        Ok(self.trades.read().await.len())
    }
}

/// In-memory implementation of PortfolioRepository
/// Stores portfolio state with equity history tracking
type EquityHistory = Vec<(DateTime<Utc>, Decimal)>;

/// In-memory implementation of PortfolioRepository
/// Stores portfolio state with equity history tracking
pub struct InMemoryPortfolioRepository {
    portfolio: Arc<RwLock<Portfolio>>,
    history: Arc<RwLock<EquityHistory>>,
}

impl InMemoryPortfolioRepository {
    pub fn new(initial: Portfolio) -> Self {
        Self {
            portfolio: Arc::new(RwLock::new(initial)),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Record a snapshot of equity at a given time
    pub async fn record_equity(&self, timestamp: DateTime<Utc>, equity: Decimal) {
        self.history.write().await.push((timestamp, equity));
    }
}

#[async_trait]
impl PortfolioRepository for InMemoryPortfolioRepository {
    async fn load(&self) -> Result<Portfolio> {
        Ok(self.portfolio.read().await.clone())
    }

    async fn save(&self, portfolio: &Portfolio) -> Result<()> {
        *self.portfolio.write().await = portfolio.clone();
        Ok(())
    }

    async fn get_equity_history(
        &self,
        start: DateTime<Utc>,
    ) -> Result<Vec<(DateTime<Utc>, Decimal)>> {
        let history = self.history.read().await;
        Ok(history
            .iter()
            .filter(|(ts, _)| ts >= &start)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::OrderSide;
    use rust_decimal_macros::dec;

    fn create_test_order(symbol: &str, side: OrderSide) -> Order {
        Order {
            id: format!("test-{}", symbol),
            symbol: symbol.to_string(),
            side,
            quantity: dec!(10),
            price: dec!(100),
            timestamp: Utc::now().timestamp(),
        }
    }

    #[tokio::test]
    async fn test_trade_repository_save_and_retrieve() {
        let repo = InMemoryTradeRepository::new();

        let order = create_test_order("AAPL", OrderSide::Buy);
        repo.save(&order).await.unwrap();

        let trades = repo.find_by_symbol("AAPL").await.unwrap();
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].symbol, "AAPL");
    }

    #[tokio::test]
    async fn test_trade_repository_find_recent() {
        let repo = InMemoryTradeRepository::new();

        for i in 0..10 {
            let order = create_test_order(&format!("SYM{}", i), OrderSide::Buy);
            repo.save(&order).await.unwrap();
        }

        let recent = repo.find_recent(3).await.unwrap();
        assert_eq!(recent.len(), 3);
        // Most recent first
        assert_eq!(recent[0].symbol, "SYM9");
        assert_eq!(recent[1].symbol, "SYM8");
        assert_eq!(recent[2].symbol, "SYM7");
    }

    #[tokio::test]
    async fn test_trade_repository_count() {
        let repo = InMemoryTradeRepository::new();

        assert_eq!(repo.count().await.unwrap(), 0);

        repo.save(&create_test_order("AAPL", OrderSide::Buy))
            .await
            .unwrap();
        repo.save(&create_test_order("TSLA", OrderSide::Sell))
            .await
            .unwrap();

        assert_eq!(repo.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_portfolio_repository_load_save() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(50000);

        let repo = InMemoryPortfolioRepository::new(portfolio.clone());

        let loaded = repo.load().await.unwrap();
        assert_eq!(loaded.cash, dec!(50000));

        // Modify and save
        let mut modified = loaded;
        modified.cash = dec!(60000);
        repo.save(&modified).await.unwrap();

        let reloaded = repo.load().await.unwrap();
        assert_eq!(reloaded.cash, dec!(60000));
    }

    #[tokio::test]
    async fn test_portfolio_repository_equity_history() {
        let portfolio = Portfolio::new();
        let repo = InMemoryPortfolioRepository::new(portfolio);

        let now = Utc::now();
        repo.record_equity(now, dec!(100000)).await;
        repo.record_equity(now + chrono::Duration::hours(1), dec!(101000))
            .await;
        repo.record_equity(now + chrono::Duration::hours(2), dec!(102000))
            .await;

        let history = repo
            .get_equity_history(now - chrono::Duration::hours(1))
            .await
            .unwrap();

        assert_eq!(history.len(), 3);
        assert_eq!(history[0].1, dec!(100000));
        assert_eq!(history[2].1, dec!(102000));
    }
}
