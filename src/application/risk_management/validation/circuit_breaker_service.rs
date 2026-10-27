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

/// Formats a ratio as a percentage for messages, without panicking on overflow.
fn as_percent(ratio: Decimal) -> String {
    match ratio.checked_mul(dec!(100)) {
        Some(pct) => pct.to_string(),
        None => format!("{} (ratio)", ratio),
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
                        as_percent(daily_loss_pct),
                        as_percent(self.config.max_daily_loss_pct),
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
                        as_percent(drawdown_pct),
                        as_percent(self.config.max_drawdown_pct)
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

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn level_of(result: Option<(HaltLevel, String)>) -> HaltLevel {
        result.map(|(level, _)| level).unwrap_or(HaltLevel::Normal)
    }

    /// Any Decimal, including extreme magnitudes and scales.
    fn any_decimal() -> impl Strategy<Value = Decimal> {
        (any::<i64>(), 0u32..=28).prop_map(|(m, scale)| Decimal::new(m, scale))
    }

    fn money() -> impl Strategy<Value = Decimal> {
        (1i64..10_000_000_000).prop_map(|c| Decimal::new(c, 2))
    }

    fn limit() -> impl Strategy<Value = Decimal> {
        (1i64..=5_000).prop_map(|bp| Decimal::new(bp, 4)) // 0.01% .. 50%
    }

    fn service(daily: Decimal, drawdown: Decimal) -> CircuitBreakerService {
        CircuitBreakerService::new(CircuitBreakerConfig {
            max_daily_loss_pct: daily,
            max_drawdown_pct: drawdown,
            consecutive_loss_limit: usize::MAX,
        })
    }

    fn state(session_start: Decimal, hwm: Decimal) -> RiskState {
        RiskState {
            session_start_equity: session_start,
            equity_high_water_mark: hwm,
            ..Default::default()
        }
    }

    proptest! {
        #[test]
        fn never_panics_on_any_input(
            start in any_decimal(),
            hwm in any_decimal(),
            equity in any_decimal(),
            daily in any_decimal(),
            drawdown in any_decimal(),
        ) {
            let _ = service(daily, drawdown).check_circuit_breaker(&state(start, hwm), equity);
        }

        #[test]
        fn lower_equity_never_lowers_the_halt_level(
            start in money(),
            hwm_extra in money(),
            e1 in money(),
            e2 in money(),
            daily in limit(),
            drawdown in limit(),
        ) {
            let cb = service(daily, drawdown);
            let s = state(start, start + hwm_extra);
            let (low, high) = if e1 <= e2 { (e1, e2) } else { (e2, e1) };
            prop_assert!(
                level_of(cb.check_circuit_breaker(&s, low))
                    >= level_of(cb.check_circuit_breaker(&s, high))
            );
        }

        #[test]
        fn no_loss_never_halts(
            start in money(),
            gain in money(),
            daily in limit(),
            drawdown in limit(),
        ) {
            // At or above both the session start and the high water mark.
            let equity = start + gain;
            let result = service(daily, drawdown).check_circuit_breaker(&state(start, equity), equity);
            prop_assert!(result.is_none());
        }

        #[test]
        fn non_positive_limit_always_halts(
            start in money(),
            equity in money(),
            bad_limit in (-5_000i64..=0).prop_map(|bp| Decimal::new(bp, 4)),
        ) {
            let result = service(bad_limit, dec!(0.10)).check_circuit_breaker(&state(start, start), equity);
            prop_assert_eq!(level_of(result), HaltLevel::FullHalt);
        }
    }
}
