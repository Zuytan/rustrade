//! Session lifecycle: initialization, daily reset and state persistence.

use super::*;

impl RiskManager {
    /// Persist current risk state to database
    pub(super) async fn persist_state(&self) {
        self.state_manager.persist().await;
    }

    /// Initialize session tracking with starting equity
    /// Delegates to SessionManager for session lifecycle management
    pub async fn initialize_session(&mut self) -> anyhow::Result<()> {
        // Wait for Portfolio Synchronization (prevents false drawdown trigger)
        info!("RiskManager: Waiting for portfolio synchronization...");
        let mut attempts = 0;
        loop {
            let snapshot = self.portfolio_state_manager.refresh().await?;
            if snapshot.portfolio.synchronized {
                info!(
                    "RiskManager: Portfolio synchronized. Proceeding with session initialization."
                );
                break;
            }

            attempts += 1;
            if attempts % 20 == 0 {
                warn!(
                    "RiskManager: Still waiting for portfolio synchronization ({}/20s)...",
                    attempts
                );
            }

            if attempts > 60 {
                // ~60 seconds timeout
                warn!(
                    "RiskManager: Portfolio synchronization timed out. Proceeding with potentially stale data."
                );
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        }

        // Get portfolio snapshot
        let snapshot = self.portfolio_state_manager.refresh().await?;

        // Delegate session initialization to SessionManager
        let risk_state = self
            .session_manager
            .initialize_session(&snapshot.portfolio, &mut self.current_prices)
            .await?;

        // Sync state manager
        *self.state_manager.get_state_mut() = risk_state.clone();

        // Push daily_start_equity to portfolio
        self.portfolio_state_manager
            .update_starting_cash(risk_state.daily_start_equity)
            .await;

        info!(
            "RiskManager: Session initialized. Equity: {}, Daily Start: {}, HWM: {}",
            self.state_manager.get_state().session_start_equity,
            self.state_manager.get_state().daily_start_equity,
            self.state_manager.get_state().equity_high_water_mark
        );

        Ok(())
    }

    /// Check if we need to reset session stats (for 24/7 Crypto markets)
    pub async fn check_daily_reset(&mut self, current_equity: Decimal) -> bool {
        let old_reset = self.state_manager.get_state().daily_drawdown_reset;

        // Delegate to RiskStateManager
        self.state_manager.check_daily_reset(current_equity);

        let new_reset = self.state_manager.get_state().daily_drawdown_reset;

        if new_reset && !old_reset {
            self.daily_pnl = Decimal::ZERO;
            self.circuit_breaker_service.set_halted(HaltLevel::Normal);
            self.metrics.circuit_breaker_status.set(0.0);

            // Push the new daily_start_equity to portfolio
            let start_equity = self.state_manager.get_state().daily_start_equity;
            self.portfolio_state_manager
                .update_starting_cash(start_equity)
                .await;

            return true;
        }

        // Check if reference date changed (handled by state manager logic above, so implied by new_reset usually)
        // But if we want to be safe about updated_at check:
        if self.asset_class == AssetClass::Crypto
            && self.state_manager.get_state().updated_at >= Utc::now().timestamp() - 1
        {
            return true;
        }
        false
    }
}
