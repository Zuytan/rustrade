## Description

<!-- Briefly describe the changes in this PR -->

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Trading strategy (new or modified trading strategy)
- [ ] Risk management (changes to risk validation or position sizing)
- [ ] Documentation update

## Testing

<!-- Describe the tests you've added or run -->

- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Manual testing performed
- [ ] All tests pass locally

## Checklist

### General Requirements

- [ ] Code follows the project's style guidelines
- [ ] I have performed a self-review of my code
- [ ] I have commented my code, particularly in hard-to-understand areas
- [ ] I have made corresponding changes to the documentation
- [ ] My changes generate no new warnings
- [ ] `cargo fmt` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes

### Trading Code Requirements ⚠️

**If this PR modifies trading strategies, risk management, or financial calculations, complete this section:**

#### Critical Blockers (Must Pass)
- [ ] Uses `rust_decimal::Decimal` for ALL currency/financial calculations
- [ ] NO use of `f64` or `f32` for money, prices, quantities, or P&L
- [ ] Dynamic risk-based position sizing implemented (NO hardcoded quantities)
- [ ] Strict stop-loss defined for all trade signals using `.with_stop_loss()`
- [ ] Signals pass through `RiskManager` validation chain
- [ ] NO direct trade execution in strategy code (strategies only return `Signal`)
- [ ] Adequate test coverage provided (unit + integration tests)
- [ ] NO look-ahead bias in strategy logic
- [ ] Edge cases tested (missing data, extreme values, zero/negative prices)

#### Quantitative Quality
- [ ] Parameter count is reasonable (<5 configurable parameters)
- [ ] Transaction costs accounted for in backtests (if applicable)
- [ ] Uses `ta` crate for standard technical indicators
- [ ] NO obvious overfitting (e.g., too many filters, excessive optimization)
- [ ] Stop losses are volatility-based (ATR) when possible, not arbitrary percentages

#### Code Quality
- [ ] NO `.unwrap()` in production code (use `?` or `.expect()` with context)
- [ ] Follows DDD architecture (domain/application/infrastructure separation)
- [ ] Public APIs have doc comments
- [ ] Ran `./scripts/review_trading_code.sh` and addressed all issues

## Related Issues

<!-- Link related issues here using #issue-number -->

Closes #

## Additional Context

<!-- Add any other context about the PR here -->

---

**For Trading Code**: This PR will be automatically reviewed by the `trading-review.yml` GitHub Action. See [REVIEW_GUIDELINES.md](../blob/main/REVIEW_GUIDELINES.md) for complete requirements.
