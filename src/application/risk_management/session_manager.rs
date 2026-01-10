//! Session Manager Service
//!
//! Handles session initialization, daily resets, and equity tracking for risk management.
//! Extracted from RiskManager to follow Single Responsibility Principle.

use crate::domain::ports::MarketDataService;
use crate::domain::repositories::RiskStateRepository;
use crate::domain::risk::state::RiskState;
use crate::domain::trading::portfolio::Portfolio;
use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Session Manager - Handles session lifecycle and equity tracking
///
/// # Responsibilities
///
/// - Initialize trading session with starting equity
/// - Track daily equity baselines for crypto 24/7 markets
/// - Load/persist session state across restarts
/// - Detect daily resets and update reference dates
pub struct SessionManager {
    risk_state_repository: Option<Arc<dyn RiskStateRepository>>,
    market_service: Arc<dyn MarketDataService>,
}

impl SessionManager {
    /// Create a new SessionManager
    pub fn new(
        risk_state_repository: Option<Arc<dyn RiskStateRepository>>,
        market_service: Arc<dyn MarketDataService>,
    ) -> Self {
        Self {
            risk_state_repository,
            market_service,
        }
    }

    /// Initialize session tracking with starting equity
    ///
    /// # Process
    ///
    /// 1. Fetch current prices for all positions
    /// 2. Calculate initial equity
    /// 3. Load persistent state (if available)
    /// 4. Restore or initialize session baselines
    /// 5. Persist state to database
    pub async fn initialize_session(
        &self,
        portfolio: &Portfolio,
        current_prices: &mut HashMap<String, Decimal>,
    ) -> Result<RiskState> {
        // Fetch initial prices for accurate equity calculation
        let symbols: Vec<String> = portfolio.positions.keys().cloned().collect();
        if !symbols.is_empty() {
            match self.market_service.get_prices(symbols).await {
                Ok(prices) => {
                    for (sym, price) in prices {
                        current_prices.insert(sym, price);
                    }
                }
                Err(e) => {
                    warn!("SessionManager: Failed to fetch initial prices: {}", e);
                }
            }
        }

        let initial_equity = portfolio.total_equity(current_prices);
        let mut risk_state = RiskState {
            id: "global".to_string(),
            session_start_equity: initial_equity,
            daily_start_equity: initial_equity,
            equity_high_water_mark: initial_equity,
            consecutive_losses: 0,
            reference_date: Utc::now().date_naive(),
            updated_at: Utc::now().timestamp(),
            daily_drawdown_reset: false,
        };

        // Attempt to load persistent state
        if let Some(repo) = &self.risk_state_repository {
            match repo.load("global").await {
                Ok(Some(state)) => {
                    info!(
                        "SessionManager: Loaded persistent state from DB: {:?}",
                        state
                    );

                    // Restore HWM and Consecutive Losses always
                    risk_state.equity_high_water_mark = state.equity_high_water_mark;
                    risk_state.consecutive_losses = state.consecutive_losses;

                    // Restore Daily/Session logic ONLY if it's the same day
                    let today = Utc::now().date_naive();
                    if state.reference_date == today {
                        risk_state.session_start_equity = state.session_start_equity;
                        risk_state.daily_start_equity = state.daily_start_equity;
                        risk_state.reference_date = state.reference_date;
                        info!(
                            "SessionManager: Restored intraday equity baselines from persistence."
                        );
                    } else {
                        info!(
                            "SessionManager: Persistent state is from previous day ({} vs {}). Using fresh equity baselines.",
                            state.reference_date, today
                        );
                        // Save the new day state immediately
                        self.persist_state(&risk_state).await;
                    }
                }
                Ok(None) => {
                    info!("SessionManager: No persistent state found. Starting fresh.");
                    self.persist_state(&risk_state).await;
                }
                Err(e) => {
                    error!(
                        "SessionManager: Failed to load persistent state: {}. Continuing with fresh state.",
                        e
                    );
                }
            }
        }

        info!(
            "SessionManager: Session initialized. Equity: {}, Daily Start: {}, HWM: {}",
            risk_state.session_start_equity,
            risk_state.daily_start_equity,
            risk_state.equity_high_water_mark
        );

        Ok(risk_state)
    }

    /// Check if we need to reset session stats (for 24/7 Crypto markets)
    ///
    /// Returns true if a daily reset occurred
    pub fn check_daily_reset(
        &self,
        current_state: &mut RiskState,
        current_equity: Decimal,
    ) -> bool {
        let today = Utc::now().date_naive();

        if current_state.reference_date < today {
            info!(
                "SessionManager: Daily reset detected. Old date: {}, New date: {}",
                current_state.reference_date, today
            );

            // Reset daily baseline
            current_state.daily_start_equity = current_equity;
            current_state.reference_date = today;
            current_state.updated_at = Utc::now().timestamp();
            current_state.daily_drawdown_reset = true;

            return true;
        }

        false
    }

    /// Persist current risk state to database
    pub async fn persist_state(&self, state: &RiskState) {
        if let Some(repo) = &self.risk_state_repository {
            if let Err(e) = repo.save(state).await {
                error!("SessionManager: Failed to persist risk state: {}", e);
            } else {
                debug!("SessionManager: Risk state persisted successfully.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::portfolio::Position;

    struct MockRiskStateRepo {
        state: Arc<tokio::sync::RwLock<Option<RiskState>>>,
    }

    #[async_trait::async_trait]
    impl RiskStateRepository for MockRiskStateRepo {
        async fn save(&self, state: &RiskState) -> Result<()> {
            *self.state.write().await = Some(state.clone());
            Ok(())
        }

        async fn load(&self, _id: &str) -> Result<Option<RiskState>> {
            Ok(self.state.read().await.clone())
        }
    }

    struct MockMarketData {
        prices: HashMap<String, Decimal>,
    }

    #[async_trait::async_trait]
    impl MarketDataService for MockMarketData {
        async fn subscribe(
            &self,
            _symbols: Vec<String>,
        ) -> Result<tokio::sync::mpsc::Receiver<crate::domain::trading::types::MarketEvent>>
        {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }

        async fn get_top_movers(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }

        async fn get_prices(&self, _symbols: Vec<String>) -> Result<HashMap<String, Decimal>> {
            Ok(self.prices.clone())
        }

        async fn get_historical_bars(
            &self,
            _symbol: &str,
            _start: chrono::DateTime<chrono::Utc>,
            _end: chrono::DateTime<chrono::Utc>,
            _timeframe: &str,
        ) -> Result<Vec<crate::domain::trading::types::Candle>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_initialize_session_fresh_start() {
        let repo = Arc::new(MockRiskStateRepo {
            state: Arc::new(tokio::sync::RwLock::new(None)),
        });

        let mut prices = HashMap::new();
        prices.insert("AAPL".to_string(), Decimal::from(150));

        let market = Arc::new(MockMarketData {
            prices: prices.clone(),
        });

        let manager = SessionManager::new(Some(repo.clone()), market);

        let mut portfolio = Portfolio::new();
        portfolio.cash = Decimal::from(10000);
        portfolio.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::from(140),
            },
        );

        let mut current_prices = HashMap::new();
        let state = manager
            .initialize_session(&portfolio, &mut current_prices)
            .await
            .unwrap();

        // Equity = 10000 cash + (10 * 150) = 11500
        let expected_equity = Decimal::from(11500);
        assert_eq!(state.session_start_equity, expected_equity);
        assert_eq!(state.daily_start_equity, expected_equity);
        assert_eq!(state.equity_high_water_mark, expected_equity);
        assert_eq!(state.consecutive_losses, 0);
    }

    #[tokio::test]
    async fn test_initialize_session_restores_hwm() {
        let existing_state = RiskState {
            id: "global".to_string(),
            session_start_equity: Decimal::from(10000),
            daily_start_equity: Decimal::from(10000),
            equity_high_water_mark: Decimal::from(12000), // Higher HWM
            consecutive_losses: 2,
            reference_date: Utc::now().date_naive(),
            updated_at: Utc::now().timestamp(),
            daily_drawdown_reset: false,
        };

        let repo = Arc::new(MockRiskStateRepo {
            state: Arc::new(tokio::sync::RwLock::new(Some(existing_state))),
        });

        let market = Arc::new(MockMarketData {
            prices: HashMap::new(),
        });

        let manager = SessionManager::new(Some(repo), market);

        let portfolio = Portfolio::new();
        let mut current_prices = HashMap::new();

        let state = manager
            .initialize_session(&portfolio, &mut current_prices)
            .await
            .unwrap();

        // Should restore HWM and consecutive losses
        assert_eq!(state.equity_high_water_mark, Decimal::from(12000));
        assert_eq!(state.consecutive_losses, 2);
    }

    #[tokio::test]
    async fn test_check_daily_reset() {
        let manager = SessionManager::new(
            None,
            Arc::new(MockMarketData {
                prices: HashMap::new(),
            }),
        );

        let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
        let mut state = RiskState {
            id: "global".to_string(),
            session_start_equity: Decimal::from(10000),
            daily_start_equity: Decimal::from(10000),
            equity_high_water_mark: Decimal::from(10000),
            consecutive_losses: 0,
            reference_date: yesterday,
            updated_at: Utc::now().timestamp(),
            daily_drawdown_reset: false,
        };

        let current_equity = Decimal::from(10500);
        let reset_occurred = manager.check_daily_reset(&mut state, current_equity);

        assert!(reset_occurred);
        assert_eq!(state.daily_start_equity, current_equity);
        assert_eq!(state.reference_date, Utc::now().date_naive());
        assert!(state.daily_drawdown_reset);
    }

    #[tokio::test]
    async fn test_no_reset_same_day() {
        let manager = SessionManager::new(
            None,
            Arc::new(MockMarketData {
                prices: HashMap::new(),
            }),
        );

        let mut state = RiskState {
            id: "global".to_string(),
            session_start_equity: Decimal::from(10000),
            daily_start_equity: Decimal::from(10000),
            equity_high_water_mark: Decimal::from(10000),
            consecutive_losses: 0,
            reference_date: Utc::now().date_naive(),
            updated_at: Utc::now().timestamp(),
            daily_drawdown_reset: false,
        };

        let current_equity = Decimal::from(10500);
        let reset_occurred = manager.check_daily_reset(&mut state, current_equity);

        assert!(!reset_occurred);
        assert_eq!(state.daily_start_equity, Decimal::from(10000)); // Unchanged
    }
}
