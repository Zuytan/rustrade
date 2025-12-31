use crate::domain::ports::ExecutionService;
use crate::domain::trading::portfolio::Portfolio;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Versioned portfolio snapshot with timestamp and reserved exposure tracking
#[derive(Debug, Clone)]
pub struct VersionedPortfolio {
    /// Monotonically increasing version number
    pub version: u64,
    
    /// Current portfolio state
    pub portfolio: Portfolio,
    
    /// Last update timestamp (milliseconds since epoch)
    pub timestamp: i64,
    
    /// Reserved capital for pending orders (reservation_id -> amount)
    pub reserved_exposure: HashMap<String, Decimal>,
}

impl VersionedPortfolio {
    /// Calculate available cash after accounting for reservations
    pub fn available_cash(&self) -> Decimal {
        let reserved_total: Decimal = self.reserved_exposure.values().sum();
        self.portfolio.cash - reserved_total
    }
}

/// Token representing a reserved exposure allocation
#[derive(Debug, Clone)]
pub struct ReservationToken {
    pub id: String,
    pub symbol: String,
    pub amount: Decimal,
}

impl ReservationToken {
    pub fn new(symbol: &str, amount: Decimal) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            symbol: symbol.to_string(),
            amount,
        }
    }
}

/// Manages portfolio state with versioning and staleness detection
///
/// This service provides:
/// - Versioned portfolio snapshots to detect stale reads
/// - Optimistic locking for conflict detection
/// - Exposure reservations to prevent over-allocation
/// - Automatic refresh with configurable staleness threshold
///
/// # Example
/// ```
/// let manager = PortfolioStateManager::new(execution_service, 5000);
///
/// // Get current snapshot
/// let snapshot = manager.get_snapshot().await;
///
/// // Check if stale
/// if manager.is_stale(&snapshot) {
///     let fresh = manager.refresh().await?;
/// }
///
/// // Reserve exposure with version check
/// let token = manager.reserve_exposure("AAPL", amount, snapshot.version).await?;
/// ```
pub struct PortfolioStateManager {
    current_state: Arc<RwLock<VersionedPortfolio>>,
    execution_service: Arc<dyn ExecutionService>,
    max_staleness_ms: i64,
}

impl PortfolioStateManager {
    /// Create a new PortfolioStateManager
    ///
    /// # Arguments
    /// * `execution_service` - Service for fetching portfolio from exchange
    /// * `max_staleness_ms` - Maximum age of snapshot before considered stale (milliseconds)
    pub fn new(execution_service: Arc<dyn ExecutionService>, max_staleness_ms: i64) -> Self {
        let initial_state = VersionedPortfolio {
            version: 0,
            portfolio: Portfolio::new(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            reserved_exposure: HashMap::new(),
        };

        Self {
            current_state: Arc::new(RwLock::new(initial_state)),
            execution_service,
            max_staleness_ms,
        }
    }

    /// Get current version number (lightweight)
    pub async fn get_version(&self) -> u64 {
        let state = self.current_state.read().await;
        state.version
    }

    /// Get full portfolio snapshot
    pub async fn get_snapshot(&self) -> VersionedPortfolio {
        let state = self.current_state.read().await;
        state.clone()
    }

    /// Check if a snapshot is stale based on timestamp
    pub fn is_stale(&self, snapshot: &VersionedPortfolio) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let age_ms = now - snapshot.timestamp;
        age_ms > self.max_staleness_ms
    }

    /// Refresh portfolio from exchange and bump version
    ///
    /// This invalidates all existing snapshots by incrementing the version.
    pub async fn refresh(&self) -> anyhow::Result<VersionedPortfolio> {
        // Fetch fresh portfolio from exchange
        let portfolio = self.execution_service.get_portfolio().await?;

        let mut state = self.current_state.write().await;
        
        // Increment version and update state
        state.version += 1;
        state.portfolio = portfolio;
        state.timestamp = chrono::Utc::now().timestamp_millis();

        info!(
            "PortfolioStateManager: Refreshed to v{} (Cash: ${}, Positions: {})",
            state.version,
            state.portfolio.cash,
            state.portfolio.positions.len()
        );

        Ok(state.clone())
    }

    /// Reserve exposure for a pending trade with optimistic locking
    ///
    /// # Arguments
    /// * `symbol` - Symbol to reserve exposure for
    /// * `amount` - Amount of capital to reserve
    /// * `expected_version` - Expected portfolio version (optimistic lock)
    ///
    /// # Returns
    /// ReservationToken on success, or error if version mismatch or insufficient funds
    pub async fn reserve_exposure(
        &self,
        symbol: &str,
        amount: Decimal,
        expected_version: u64,
    ) -> anyhow::Result<ReservationToken> {
        let mut state = self.current_state.write().await;

        // Optimistic lock: check version
        if state.version != expected_version {
            return Err(anyhow::anyhow!(
                "Version conflict: expected v{}, actual v{} (portfolio changed)",
                expected_version,
                state.version
            ));
        }

        // Calculate available cash
        let available = state.available_cash();

        if available < amount {
            return Err(anyhow::anyhow!(
                "Insufficient funds: need ${}, available ${} (reserved: ${})",
                amount,
                available,
                state.reserved_exposure.values().sum::<Decimal>()
            ));
        }

        // Create reservation
        let token = ReservationToken::new(symbol, amount);
        state.reserved_exposure.insert(token.id.clone(), amount);

        info!(
            "PortfolioStateManager: Reserved ${} for {} (token: {}, v{})",
            amount, symbol, &token.id[..8], state.version
        );

        Ok(token)
    }

    /// Release a reservation (trade completed or cancelled)
    pub async fn release_reservation(&self, token: ReservationToken) {
        let mut state = self.current_state.write().await;
        
        if state.reserved_exposure.remove(&token.id).is_some() {
            info!(
                "PortfolioStateManager: Released ${} for {} (token: {})",
                token.amount, token.symbol, &token.id[..8]
            );
        } else {
            warn!(
                "PortfolioStateManager: Attempted to release non-existent reservation (token: {})",
                &token.id[..8]
            );
        }
    }

    /// Get total reserved exposure
    pub async fn get_total_reserved(&self) -> Decimal {
        let state = self.current_state.read().await;
        state.reserved_exposure.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::OrderUpdate;
    use async_trait::async_trait;
    use rust_decimal_macros::dec;

    struct MockExecutionService {
        portfolio: Arc<RwLock<Portfolio>>,
    }

    #[async_trait]
    impl ExecutionService for MockExecutionService {
        async fn execute(&self, _order: crate::domain::trading::types::Order) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_portfolio(&self) -> anyhow::Result<Portfolio> {
            let portfolio = self.portfolio.read().await;
            Ok(portfolio.clone())
        }

        async fn get_today_orders(&self) -> anyhow::Result<Vec<crate::domain::trading::types::Order>> {
            Ok(Vec::new())
        }

        async fn subscribe_order_updates(
            &self,
        ) -> anyhow::Result<tokio::sync::broadcast::Receiver<OrderUpdate>> {
            let (tx, _rx) = tokio::sync::broadcast::channel(1);
            Ok(tx.subscribe())
        }
    }

    #[tokio::test]
    async fn test_version_increments_on_refresh() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(10000);
        
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = PortfolioStateManager::new(mock_service, 5000);

        let v1 = manager.get_version().await;
        assert_eq!(v1, 0);

        manager.refresh().await.unwrap();
        let v2 = manager.get_version().await;
        assert_eq!(v2, 1);

        manager.refresh().await.unwrap();
        let v3 = manager.get_version().await;
        assert_eq!(v3, 2);
    }

    #[tokio::test]
    async fn test_stale_detection() {
        let portfolio = Portfolio::new();
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = PortfolioStateManager::new(mock_service, 100); // 100ms threshold

        let snapshot = manager.get_snapshot().await;
        assert!(!manager.is_stale(&snapshot));

        // Wait 150ms
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        assert!(manager.is_stale(&snapshot));
    }

    #[tokio::test]
    async fn test_reserve_exposure_succeeds() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(10000);
        
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = PortfolioStateManager::new(mock_service, 5000);
        
        // Refresh to load portfolio from mock
        manager.refresh().await.unwrap();
        let snapshot = manager.get_snapshot().await;

        let token = manager
            .reserve_exposure("AAPL", dec!(3000), snapshot.version)
            .await
            .unwrap();

        assert_eq!(token.amount, dec!(3000));
        assert_eq!(token.symbol, "AAPL");

        let reserved = manager.get_total_reserved().await;
        assert_eq!(reserved, dec!(3000));
    }

    #[tokio::test]
    async fn test_reserve_exposure_version_mismatch() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(10000);
        
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = PortfolioStateManager::new(mock_service, 5000);
        let snapshot = manager.get_snapshot().await;

        // Refresh to bump version
        manager.refresh().await.unwrap();

        // Try to reserve with old version
        let result = manager
            .reserve_exposure("AAPL", dec!(3000), snapshot.version)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Version conflict"));
    }

    #[tokio::test]
    async fn test_reserve_exposure_insufficient_funds() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(5000);
        
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = PortfolioStateManager::new(mock_service, 5000);
        let snapshot = manager.get_snapshot().await;

        let result = manager
            .reserve_exposure("AAPL", dec!(10000), snapshot.version)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Insufficient funds"));
    }

    #[tokio::test]
    async fn test_release_reservation() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(10000);
        
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = PortfolioStateManager::new(mock_service, 5000);
        
        // Refresh to load portfolio from mock
        manager.refresh().await.unwrap();
        let snapshot = manager.get_snapshot().await;

        let token = manager
            .reserve_exposure("AAPL", dec!(3000), snapshot.version)
            .await
            .unwrap();

        assert_eq!(manager.get_total_reserved().await, dec!(3000));

        manager.release_reservation(token).await;

        assert_eq!(manager.get_total_reserved().await, dec!(0));
    }

    #[tokio::test]
    async fn test_concurrent_reservations() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(10000);
        
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = Arc::new(PortfolioStateManager::new(mock_service, 5000));
        
        // Refresh to load portfolio from mock
        manager.refresh().await.unwrap();
        let snapshot = manager.get_snapshot().await;

        // Reserve  3 times $3000 = $9000 (should all succeed)
        let manager1 = manager.clone();
        let manager2 = manager.clone();
        let manager3 = manager.clone();
        let v = snapshot.version;

        let t1 = tokio::spawn(async move {
            manager1.reserve_exposure("AAPL", dec!(3000), v).await
        });

        let t2 = tokio::spawn(async move {
            manager2.reserve_exposure("MSFT", dec!(3000), v).await
        });

        let t3 = tokio::spawn(async move {
            manager3.reserve_exposure("TSLA", dec!(3000), v).await
        });

        let r1 = t1.await.unwrap();
        let r2 = t2.await.unwrap();
        let r3 = t3.await.unwrap();

        // All should succeed
        assert!(r1.is_ok());
        assert!(r2.is_ok());
        assert!(r3.is_ok());

        let total = manager.get_total_reserved().await;
        assert_eq!(total, dec!(9000));
    }

    #[tokio::test]
    async fn test_available_cash_calculation() {
        let mut portfolio = Portfolio::new();
        portfolio.cash = dec!(10000);
        
        let mock_service = Arc::new(MockExecutionService {
            portfolio: Arc::new(RwLock::new(portfolio)),
        });

        let manager = PortfolioStateManager::new(mock_service, 5000);
        
        // Refresh to load portfolio from mock
        manager.refresh().await.unwrap();
        let snapshot = manager.get_snapshot().await;

        // Initially all cash available
        assert_eq!(snapshot.available_cash(), dec!(10000));

        // Reserve $3000
        manager
            .reserve_exposure("AAPL", dec!(3000), snapshot.version)
            .await
            .unwrap();

        let snapshot2 = manager.get_snapshot().await;
        assert_eq!(snapshot2.available_cash(), dec!(7000));
    }
}
