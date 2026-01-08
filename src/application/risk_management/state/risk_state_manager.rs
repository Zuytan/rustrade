use crate::domain::risk::state::RiskState;
use crate::domain::repositories::RiskStateRepository;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{info, warn};

/// Manages the persistence and updates of global risk state
/// like High Water Mark, Daily Loss, and Consecutive Losses.
pub struct RiskStateManager {
    risk_state: RiskState,
    repository: Option<Arc<dyn RiskStateRepository>>,
}

impl RiskStateManager {
    pub fn new(
        repository: Option<Arc<dyn RiskStateRepository>>,
        initial_equity: Decimal,
    ) -> Self {
        let mut manager = Self {
            risk_state: RiskState::default(),
            repository,
        };

        // Initialize state
        manager.initialize(initial_equity);
        manager
    }

    fn initialize(&mut self, current_equity: Decimal) {
        // Try to load persisted state
        if let Some(repo) = &self.repository {
            match futures::executor::block_on(repo.load("global")) {
                Ok(Some(state)) => {
                    self.risk_state = state;
                    info!("Loaded persisted risk state: {:?}", self.risk_state);
                }
                Ok(None) => {
                    info!("No persisted risk state found, starting fresh.");
                    self.risk_state = RiskState::default();
                    self.risk_state.session_start_equity = current_equity;
                    self.risk_state.equity_high_water_mark = current_equity;
                }
                Err(e) => {
                    warn!("Failed to load risk state: {}. Starting fresh.", e);
                    self.risk_state = RiskState::default();
                    self.risk_state.session_start_equity = current_equity;
                    self.risk_state.equity_high_water_mark = current_equity;
                }
            }
        }
        
        // Always verify if daily reset is needed on startup
        self.check_daily_reset(current_equity);
    }
    
    /// Get reference to current state
    pub fn get_state(&self) -> &RiskState {
        &self.risk_state
    }
    
    /// Get mutable reference (careful!)
    pub fn get_state_mut(&mut self) -> &mut RiskState {
        &mut self.risk_state
    }

    /// Update HWM and potential daily reset logic
    pub async fn update(&mut self, current_equity: Decimal, timestamp: DateTime<Utc>) {
        // Update High Water Mark
        if current_equity > self.risk_state.equity_high_water_mark {
            self.risk_state.equity_high_water_mark = current_equity;
        }

        // Check for daily reset
        self.check_daily_reset(current_equity);
        
        // Persist state
        if let Some(repo) = &self.repository {
            // Update timestamp before saving
            self.risk_state.updated_at = timestamp.timestamp();
            if let Err(e) = repo.save(&self.risk_state).await {
                warn!("Failed to persist risk state: {}", e);
            }
        }
    }
    
    /// Check if a new trading day has started and reset daily metrics
    pub fn check_daily_reset(&mut self, current_equity: Decimal) {
        let now = Utc::now();
        let last_update = DateTime::<Utc>::from_timestamp(self.risk_state.updated_at, 0)
            .unwrap_or(Utc::now());

        // Simple check: if day of year changed
        if now.date_naive() > last_update.date_naive() {
            info!(
                "New trading day detected ({}). Resetting daily session metrics.",
                now
            );
            self.risk_state.session_start_equity = current_equity;
            self.risk_state.daily_drawdown_reset = true;
            self.risk_state.updated_at = now.timestamp();
            self.risk_state.reference_date = now.date_naive();
        }
    }
    
    /// Record a loss (increments consecutive losses)
    pub async fn record_loss(&mut self) {
        self.risk_state.consecutive_losses += 1;
        self.persist().await;
    }
    
    /// Record a win (resets consecutive losses)
    pub async fn record_win(&mut self) {
        if self.risk_state.consecutive_losses > 0 {
            info!("Win recorded, resetting consecutive losses to 0");
            self.risk_state.consecutive_losses = 0;
            self.persist().await;
        }
    }
    
    pub async fn persist(&self) {
        if let Some(repo) = &self.repository 
            && let Err(e) = repo.save(&self.risk_state).await {
                warn!("Failed to persist risk state: {}", e);
        }
    }
}
