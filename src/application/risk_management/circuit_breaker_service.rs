use crate::domain::risk::state::RiskState;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive; // Ensure path is correct. If it fails, check domain structure.
// Actually, RiskState is likely re-exported or strictly under state.
// verified in risk_manager.rs: use crate::domain::risk::state::RiskState;

#[derive(Clone, Debug)]
pub struct CircuitBreakerConfig {
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub consecutive_loss_limit: usize,
}

pub struct CircuitBreakerService {
    config: CircuitBreakerConfig,
    halted: bool,
}

impl CircuitBreakerService {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            halted: false,
        }
    }

    /// Check if circuit breaker should trigger
    pub fn check_circuit_breaker(
        &self,
        risk_state: &RiskState,
        current_equity: Decimal,
    ) -> Option<String> {
        // Check daily loss limit
        if risk_state.session_start_equity > Decimal::ZERO {
            let daily_loss_pct = ((current_equity - risk_state.session_start_equity)
                / risk_state.session_start_equity)
                .to_f64()
                .unwrap_or(0.0);

            if daily_loss_pct < -self.config.max_daily_loss_pct {
                return Some(format!(
                    "Daily loss limit breached: {:.2}% (limit: {:.2}%) [Start: {}, Current: {}]",
                    daily_loss_pct * 100.0,
                    self.config.max_daily_loss_pct * 100.0,
                    risk_state.session_start_equity,
                    current_equity
                ));
            }
        }

        // Check drawdown limit
        if risk_state.equity_high_water_mark > Decimal::ZERO {
            let drawdown_pct = ((current_equity - risk_state.equity_high_water_mark)
                / risk_state.equity_high_water_mark)
                .to_f64()
                .unwrap_or(0.0);

            if drawdown_pct < -self.config.max_drawdown_pct {
                return Some(format!(
                    "Max drawdown breached: {:.2}% (limit: {:.2}%)",
                    drawdown_pct * 100.0,
                    self.config.max_drawdown_pct * 100.0
                ));
            }
        }

        // Check consecutive losses
        if risk_state.consecutive_losses >= self.config.consecutive_loss_limit {
            return Some(format!(
                "Consecutive loss limit reached: {} trades (limit: {})",
                risk_state.consecutive_losses, self.config.consecutive_loss_limit
            ));
        }

        None
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn set_halted(&mut self, halted: bool) {
        self.halted = halted;
    }
}
