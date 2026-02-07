use crate::domain::risk::state::RiskState;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

#[derive(Clone, Debug)]
pub struct CircuitBreakerConfig {
    pub max_daily_loss_pct: Decimal,
    pub max_drawdown_pct: Decimal,
    pub consecutive_loss_limit: usize,
}

/// Progressive halt level: 50% of limit = Warning (reduce size), 75% = Reduced (no new trades), 100% = FullHalt (liquidation).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HaltLevel {
    Normal = 0,
    Warning = 1,
    Reduced = 2,
    FullHalt = 3,
}

impl HaltLevel {
    pub fn is_any_halt(self) -> bool {
        self != HaltLevel::Normal
    }

    /// Position size multiplier when in this level (1.0 = full, 0.5 = half, 0.0 = no new)
    pub fn size_multiplier(self) -> f64 {
        match self {
            HaltLevel::Normal => 1.0,
            HaltLevel::Warning => 0.5,
            HaltLevel::Reduced => 0.0,
            HaltLevel::FullHalt => 0.0,
        }
    }
}

pub struct CircuitBreakerService {
    config: CircuitBreakerConfig,
    level: HaltLevel,
}

impl CircuitBreakerService {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            level: HaltLevel::Normal,
        }
    }

    /// Check circuit breaker; returns highest level triggered and message.
    pub fn check_circuit_breaker(
        &self,
        risk_state: &RiskState,
        current_equity: Decimal,
    ) -> Option<(HaltLevel, String)> {
        let mut max_level = HaltLevel::Normal;
        let mut msg = String::new();

        if risk_state.session_start_equity > Decimal::ZERO {
            let daily_loss_pct = (current_equity - risk_state.session_start_equity)
                .checked_div(risk_state.session_start_equity)
                .unwrap_or(Decimal::ZERO);
            let ratio = (daily_loss_pct
                .checked_div(-self.config.max_daily_loss_pct)
                .unwrap_or(Decimal::ZERO))
            .to_f64()
            .unwrap_or(0.0);
            let level = if ratio >= 1.0 {
                HaltLevel::FullHalt
            } else if ratio >= 0.75 {
                HaltLevel::Reduced
            } else if ratio >= 0.5 {
                HaltLevel::Warning
            } else {
                HaltLevel::Normal
            };
            if level != HaltLevel::Normal {
                let m = format!(
                    "Daily loss {}% (limit {}%) [Start: {}, Current: {}]",
                    daily_loss_pct * dec!(100),
                    self.config.max_daily_loss_pct * dec!(100),
                    risk_state.session_start_equity,
                    current_equity
                );
                if level > max_level {
                    max_level = level;
                    msg = m;
                }
            }
        }

        if risk_state.equity_high_water_mark > Decimal::ZERO {
            let drawdown_pct = (current_equity - risk_state.equity_high_water_mark)
                .checked_div(risk_state.equity_high_water_mark)
                .unwrap_or(Decimal::ZERO);
            let ratio = (drawdown_pct
                .checked_div(-self.config.max_drawdown_pct)
                .unwrap_or(Decimal::ZERO))
            .to_f64()
            .unwrap_or(0.0);
            let level = if ratio >= 1.0 {
                HaltLevel::FullHalt
            } else if ratio >= 0.75 {
                HaltLevel::Reduced
            } else if ratio >= 0.5 {
                HaltLevel::Warning
            } else {
                HaltLevel::Normal
            };
            if level != HaltLevel::Normal {
                let m = format!(
                    "Max drawdown {}% (limit {}%)",
                    drawdown_pct * dec!(100),
                    self.config.max_drawdown_pct * dec!(100)
                );
                if level > max_level {
                    max_level = level;
                    msg = m;
                }
            }
        }

        if risk_state.consecutive_losses >= self.config.consecutive_loss_limit {
            let m = format!(
                "Consecutive loss limit reached: {} trades (limit: {})",
                risk_state.consecutive_losses, self.config.consecutive_loss_limit
            );
            if HaltLevel::FullHalt > max_level {
                max_level = HaltLevel::FullHalt;
                msg = m;
            }
        }

        if max_level != HaltLevel::Normal {
            Some((max_level, msg))
        } else {
            None
        }
    }

    pub fn is_halted(&self) -> bool {
        self.level.is_any_halt()
    }

    pub fn halt_level(&self) -> HaltLevel {
        self.level
    }

    pub fn set_halted(&mut self, level: HaltLevel) {
        self.level = level;
    }
}
