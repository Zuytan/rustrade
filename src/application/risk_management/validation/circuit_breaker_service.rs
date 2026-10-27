use crate::domain::risk::state::RiskState;
use rust_decimal::Decimal;
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

/// Halt level for an equity change against a reference and a positive loss limit.
///
/// Returns the level and the signed change (e.g. -0.015 for -1.5%), or `None` when it
/// cannot be computed (non-positive limit, arithmetic overflow). Callers must treat
/// `None` as a full halt: a safety check that cannot be evaluated must not pass.
fn loss_level(
    current_equity: Decimal,
    reference_equity: Decimal,
    limit_pct: Decimal,
) -> Option<(HaltLevel, Decimal)> {
    if limit_pct <= Decimal::ZERO {
        return None;
    }
    let change_pct = current_equity
        .checked_sub(reference_equity)?
        .checked_div(reference_equity)?;
    let ratio = change_pct.checked_div(-limit_pct)?;
    let level = if ratio >= dec!(1.0) {
        HaltLevel::FullHalt
    } else if ratio >= dec!(0.75) {
        HaltLevel::Reduced
    } else if ratio >= dec!(0.5) {
        HaltLevel::Warning
    } else {
        HaltLevel::Normal
    };
    Some((level, change_pct))
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
            let (level, m) = match loss_level(
                current_equity,
                risk_state.session_start_equity,
                self.config.max_daily_loss_pct,
            ) {
                Some((level, daily_loss_pct)) => (
                    level,
                    format!(
                        "Daily loss {}% (limit {}%) [Start: {}, Current: {}]",
                        daily_loss_pct * dec!(100),
                        self.config.max_daily_loss_pct * dec!(100),
                        risk_state.session_start_equity,
                        current_equity
                    ),
                ),
                None => (
                    HaltLevel::FullHalt,
                    format!(
                        "Daily loss cannot be evaluated (limit {}, start {}, current {}): halting",
                        self.config.max_daily_loss_pct,
                        risk_state.session_start_equity,
                        current_equity
                    ),
                ),
            };
            if level > max_level {
                max_level = level;
                msg = m;
            }
        }

        if risk_state.equity_high_water_mark > Decimal::ZERO {
            let (level, m) = match loss_level(
                current_equity,
                risk_state.equity_high_water_mark,
                self.config.max_drawdown_pct,
            ) {
                Some((level, drawdown_pct)) => (
                    level,
                    format!(
                        "Max drawdown {}% (limit {}%)",
                        drawdown_pct * dec!(100),
                        self.config.max_drawdown_pct * dec!(100)
                    ),
                ),
                None => (
                    HaltLevel::FullHalt,
                    format!(
                        "Drawdown cannot be evaluated (limit {}, high water mark {}, current {}): halting",
                        self.config.max_drawdown_pct,
                        risk_state.equity_high_water_mark,
                        current_equity
                    ),
                ),
            };
            if level > max_level {
                max_level = level;
                msg = m;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn service(max_daily_loss_pct: Decimal, max_drawdown_pct: Decimal) -> CircuitBreakerService {
        CircuitBreakerService::new(CircuitBreakerConfig {
            max_daily_loss_pct,
            max_drawdown_pct,
            consecutive_loss_limit: 100,
        })
    }

    fn state(session_start: Decimal, hwm: Decimal) -> RiskState {
        RiskState {
            session_start_equity: session_start,
            equity_high_water_mark: hwm,
            ..Default::default()
        }
    }

    #[test]
    fn test_no_loss_is_normal() {
        let cb = service(dec!(0.02), dec!(0.10));
        assert!(
            cb.check_circuit_breaker(&state(dec!(1000), dec!(1000)), dec!(1000))
                .is_none()
        );
    }

    #[test]
    fn test_daily_loss_levels() {
        let cb = service(dec!(0.02), dec!(0.50));
        let s = state(dec!(1000), dec!(1000));
        // -1% = 50% of the 2% limit
        assert_eq!(
            cb.check_circuit_breaker(&s, dec!(990)).map(|r| r.0),
            Some(HaltLevel::Warning)
        );
        // -1.5% = 75% of the limit
        assert_eq!(
            cb.check_circuit_breaker(&s, dec!(985)).map(|r| r.0),
            Some(HaltLevel::Reduced)
        );
        // -2% = limit reached
        assert_eq!(
            cb.check_circuit_breaker(&s, dec!(980)).map(|r| r.0),
            Some(HaltLevel::FullHalt)
        );
    }

    #[test]
    fn test_zero_daily_limit_fails_closed() {
        // A zero limit cannot be evaluated: it must halt, not silently pass.
        let cb = service(Decimal::ZERO, dec!(0.10));
        let result = cb.check_circuit_breaker(&state(dec!(1000), dec!(1000)), dec!(1000));
        let (level, msg) = result.expect("must halt");
        assert_eq!(level, HaltLevel::FullHalt);
        assert!(msg.contains("cannot be evaluated"));
    }

    #[test]
    fn test_zero_drawdown_limit_fails_closed() {
        let cb = service(dec!(0.02), Decimal::ZERO);
        let result = cb.check_circuit_breaker(&state(dec!(1000), dec!(1000)), dec!(1000));
        assert_eq!(result.map(|r| r.0), Some(HaltLevel::FullHalt));
    }

    #[test]
    fn test_negative_limit_fails_closed() {
        let cb = service(dec!(-0.02), dec!(0.10));
        let result = cb.check_circuit_breaker(&state(dec!(1000), dec!(1000)), dec!(1000));
        assert_eq!(result.map(|r| r.0), Some(HaltLevel::FullHalt));
    }

    #[test]
    fn test_overflowing_ratio_fails_closed() {
        // Tiny reference equity with a huge loss overflows the division: must halt.
        let cb = service(dec!(0.0000000000000000000000000001), dec!(0.10));
        let result = cb.check_circuit_breaker(
            &state(dec!(0.0000000000000000000000000001), Decimal::ZERO),
            Decimal::MIN,
        );
        assert_eq!(result.map(|r| r.0), Some(HaltLevel::FullHalt));
    }

    #[test]
    fn test_uninitialized_session_is_skipped() {
        let cb = service(dec!(0.02), dec!(0.10));
        assert!(
            cb.check_circuit_breaker(&state(Decimal::ZERO, Decimal::ZERO), dec!(500))
                .is_none()
        );
    }
}
