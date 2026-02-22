---
name: Trading Code Review
description: Strict code review for trading algorithms and risk management
---

# Skill: Trading Code Review

## When to use this skill

- **Before merging** any PR that modifies trading strategies
- **Before merging** any PR that modifies risk management logic
- **Before merging** any PR with financial calculations
- **When reviewing** Pull Requests as a maintainer
- **After implementing** a new trading feature (self-review)

## Purpose

This skill enforces a **zero-tolerance policy** for financial safety violations. The goal is to prevent:
- Financial losses due to floating-point errors
- Uncontrolled risk from missing stop losses
- Over-optimized strategies that fail in live trading
- Architectural violations that bypass risk controls

## Review Authority

As a reviewer using this skill, you have the authority to:
- **REJECT** PRs with critical blockers
- **REQUEST CHANGES** for warnings and concerns
- **REQUIRE FIXES** before approval
- **DEMAND TESTS** for untested code

Be uncompromising on financial safety. Better to be overly cautious than to deploy risky code.

## Review Process

### Step 1: Automated Checks

Run the automated review script:

```bash
./scripts/review_trading_code.sh
```

This checks for:
- ⛔ Float types in financial code (BLOCKER)
- ⛔ Direct order execution in strategies (BLOCKER)
- 🟡 Missing stop losses (WARNING)
- 🟡 Hardcoded quantities (WARNING)
- 🟡 .unwrap() usage (WARNING)
- 🟡 Missing tests (WARNING)

### Step 2: Manual Code Review

Review the code changes for:

#### 2.1 Monetary Precision (CRITICAL)
- [ ] All prices use `Decimal`, not `f64`
- [ ] All quantities use `Decimal`, not `f64`
- [ ] All P&L calculations use `Decimal`
- [ ] No conversions from `f64` to `Decimal` in hot paths

#### 2.2 Risk Management (CRITICAL)
- [ ] Every signal has a stop loss defined
- [ ] Stop losses are volatility-based (ATR preferred)
- [ ] Position sizing is dynamic and risk-based
- [ ] Risk per trade is capped (1-2% of capital)
- [ ] Strategies pass through RiskManager

#### 2.3 Quantitative Integrity
- [ ] No excessive parameters (flag if >5)
- [ ] No look-ahead bias in backtests
- [ ] Transaction costs included in simulations
- [ ] Realistic assumptions about fills and slippage

#### 2.4 Architecture
- [ ] Strategies only return `Signal`, not execute orders
- [ ] Flow: Analyst → RiskManager → Executor
- [ ] Uses `ta` crate for standard indicators
- [ ] Proper DDD separation (domain/application/infrastructure)

#### 2.5 Testing
- [ ] Unit tests for signal generation logic
- [ ] Integration tests for full flow
- [ ] Edge cases tested (None, zero, negative values)
- [ ] Test coverage >70% for new code

### Step 3: Generate Review Report

Use the review report template:

```markdown
# Code Review: [PR Title]

## 🔴 CRITICAL BLOCKERS

[List any zero-tolerance violations]

- [ ] None

OR

- ⛔ **Float types in financial calculations** (line X in file Y)
- ⛔ **Direct order execution in strategy** (line X in file Y)

## 🟡 QUANT/RISK WARNINGS

[List concerns that should be addressed]

- 🟡 **Parameter bloat**: Strategy has 7 configurable parameters
- 🟡 **Missing transaction costs**: Backtest doesn't account for slippage

## 🟢 STRUCTURAL SUGGESTIONS

[Optional improvements]

- Consider extracting common logic to helper function
- Add doc comments for public API

## ✅ CHECKLIST

- [x] Uses `rust_decimal::Decimal` for all currency
- [x] Dynamic risk-based position sizing implemented
- [x] Strict stop-loss defined
- [x] Passes through `RiskManager`
- [ ] Adequate test coverage provided

## VERDICT

- [ ] APPROVE - All requirements met
- [x] REQUEST CHANGES - Critical blockers present
- [ ] COMMENT - Warnings only, use judgment

## Required Actions

1. Replace all f64 with Decimal in src/strategies/my_strategy.rs
2. Add unit tests for edge cases
3. Document the rationale for 7 parameters
```

## Critical Rules Reference

### Rule 1: No Float Types for Money (BLOCKER)

```rust
// ❌ REJECT
let price: f64 = 123.45;

// ✅ APPROVE
use rust_decimal::Decimal;
let price = Decimal::from_str("123.45").unwrap();
```

### Rule 2: Stop Losses Required (BLOCKER)

```rust
// ❌ REJECT
Signal::buy("Buy signal")

// ✅ APPROVE
Signal::buy("Buy signal")
    .with_stop_loss(entry_price - atr * Decimal::from(2))
```

### Rule 3: Dynamic Position Sizing (BLOCKER)

```rust
// ❌ REJECT
let quantity = Decimal::from(100);

// ✅ APPROVE
let risk_amount = capital * risk_pct;
let quantity = risk_amount / stop_distance;
```

### Rule 4: No Direct Execution (BLOCKER)

```rust
// ❌ REJECT - Strategy executes directly
impl TradingStrategy for MyStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        executor.place_order(...);  // WRONG!
    }
}

// ✅ APPROVE - Strategy returns signal
impl TradingStrategy for MyStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        Some(Signal::buy("reason"))  // Correct!
    }
}
```

### Rule 5: Tests Required (WARNING)

Every strategy must have:
- Unit tests for signal logic
- Integration tests for full flow
- Edge case tests (None values, extremes)

## Templates

### Quick Review Checklist

See: `templates/quick_review.md`

### Detailed Review Report

See: `templates/detailed_review.md`

## Tools

### Automated Script

```bash
./scripts/review_trading_code.sh [path]
```

### GitHub Action

The `trading-review.yml` workflow runs automatically on PRs that modify:
- `src/application/strategies/**`
- `src/application/risk_management/**`
- `src/domain/trading/**`
- `src/domain/risk/**`

### Clippy Configuration

Custom clippy rules in `.cargo/clippy.toml` enforce:
- No unwrap without expect
- Float comparison warnings
- Precision loss warnings

## Best Practices

1. **Review early**: Catch issues before implementation is complete
2. **Be specific**: Point to exact lines and files
3. **Explain why**: Don't just say "wrong", explain the risk
4. **Suggest fixes**: Provide example code when possible
5. **Be constructive**: The goal is to help, not to block
6. **Zero tolerance**: But only for true financial risks

## Common Patterns to Flag

### Anti-Pattern 1: Over-Optimization

```rust
// TOO MANY PARAMETERS - Flag for overfitting
pub struct OverfittedStrategy {
    pub sma_fast: usize,
    pub sma_slow: usize,
    pub ema_fast: usize,
    pub rsi_period: usize,
    pub rsi_overbought: f64,
    pub rsi_oversold: f64,
    pub macd_fast: usize,
    pub macd_slow: usize,
    // ... 10 more parameters
}
```

### Anti-Pattern 2: Look-Ahead Bias

```rust
// Uses future data not available at decision time
let future_high = ctx.candles.back().unwrap().high;
if current_price < future_high {
    // This is impossible to know in real-time!
}
```

### Anti-Pattern 3: Perfect Fill Assumptions

```rust
// Assumes perfect fill at close price
let entry_price = last_candle.close;  // Unrealistic
// Should model slippage and partial fills
```

## Reference Documentation

- **Full Guidelines**: `REVIEW_GUIDELINES.md` (root)
- **Trading Rules**: `.agent/skills/rust-trading/SKILL.md`
- **Critical Review**: `.agent/skills/critical-review/SKILL.md`
- **Contributing**: `CONTRIBUTING.md`

## Escalation

If you encounter:
- **Unclear requirements**: Ask in PR comments
- **Disagreement on severity**: Discuss with team
- **Novel trading concepts**: Request quantitative validation
- **Security concerns**: Flag immediately, don't merge

## Remember

> "In trading systems, bugs cost money. Be thorough, be critical, be precise."

The cost of a false positive (rejecting good code) is low.
The cost of a false negative (accepting bad code) can be catastrophic.

**When in doubt, request changes.**
