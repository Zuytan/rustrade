# Detailed Trading Code Review Report

**PR**: [Link to PR]  
**Reviewer**: [Your Name]  
**Date**: [Date]  
**Files Changed**: [List main files]

---

## Executive Summary

[Brief overview of the changes and overall assessment]

---

## 🔴 CRITICAL BLOCKERS

> ⛔ These MUST be fixed before merge. PR **CANNOT** be approved with blockers.

### Blocker 1: [Title]

**Severity**: CRITICAL  
**Location**: `src/path/to/file.rs:123`

**Issue**:
```rust
// Current code (WRONG)
let price: f64 = 123.45;
```

**Why This is Critical**:
Floating-point types cause rounding errors that lead to financial losses.

**Required Fix**:
```rust
// Required fix
use rust_decimal::Decimal;
let price = Decimal::from_str("123.45").unwrap();
```

**Action Required**: Replace all f64/f32 with Decimal in financial calculations.

---

### Blocker 2: [Title]

[Follow same format for additional blockers]

---

## 🟡 QUANT/RISK WARNINGS

> These should be addressed but are not blocking if justification provided.

### Warning 1: [Title]

**Severity**: WARNING  
**Location**: `src/path/to/file.rs:456`

**Concern**:
Strategy has 7 configurable parameters, suggesting potential overfitting.

**Recommendation**:
- Reduce to 3-4 core parameters
- Provide out-of-sample validation
- Document why each parameter is necessary

**Risk**: High risk of overfitting to historical data, poor live performance.

---

### Warning 2: [Title]

[Follow same format for additional warnings]

---

## 🟢 STRUCTURAL & RUST SUGGESTIONS

> Optional improvements for code quality.

### Suggestion 1: [Title]

**Location**: `src/path/to/file.rs:789`

**Current**:
```rust
if let Some(x) = value {
    x.unwrap()
}
```

**Suggested**:
```rust
value.flatten()
```

**Benefit**: More idiomatic, clearer intent.

---

## ✅ REVIEW CHECKLIST

### Critical Blockers
- [ ] Uses `rust_decimal::Decimal` for all currency calculations
- [ ] Dynamic risk-based position sizing implemented
- [ ] Strict stop-loss defined for all trade signals
- [ ] Signals pass through `RiskManager` validation chain
- [ ] NO direct trade execution in strategy code
- [ ] NO look-ahead bias in strategy logic

### Quantitative Quality
- [ ] Parameter count reasonable (<5)
- [ ] Transaction costs accounted for
- [ ] Edge cases handled (None, zero, negative)
- [ ] Uses `ta` crate for standard indicators
- [ ] No obvious overfitting red flags

### Code Quality
- [ ] Adequate test coverage (unit + integration)
- [ ] NO `.unwrap()` in production code
- [ ] Follows DDD architecture
- [ ] Public APIs documented
- [ ] Passes `cargo clippy -- -D warnings`
- [ ] Passes `cargo fmt --check`
- [ ] Passes `cargo test`

---

## TEST COVERAGE ANALYSIS

**Files Modified**: X  
**Test Files Added/Updated**: Y  
**Coverage Estimate**: Z%

**Gaps Identified**:
- [ ] Missing tests for [scenario]
- [ ] No edge case tests for [condition]
- [ ] Integration test needed for [flow]

---

## QUANTITATIVE ANALYSIS

### Strategy Parameters

| Parameter | Value | Justification | Overfitting Risk |
|-----------|-------|---------------|------------------|
| sma_period | 20 | Standard setting | Low |
| ... | ... | ... | ... |

**Assessment**: [Low/Medium/High risk of overfitting]

### Risk Management

- **Max Risk per Trade**: [X%]
- **Stop Loss Method**: [ATR-based / Fixed %]
- **Position Sizing**: [Risk-based / Fixed]

**Assessment**: [Adequate / Needs improvement]

### Backtesting Assumptions

- [ ] Commission costs included
- [ ] Slippage modeled
- [ ] Realistic fill assumptions
- [ ] Out-of-sample validation

**Assessment**: [Realistic / Optimistic / Unknown]

---

## ARCHITECTURAL COMPLIANCE

**Signal Flow**:
```
Strategy → [?] → RiskManager → [?] → Executor
```

- [ ] Correct separation of concerns
- [ ] Proper DDD layering
- [ ] No circular dependencies

---

## SECURITY & SAFETY

- [ ] No unsafe code
- [ ] No panics possible
- [ ] Error handling complete
- [ ] Input validation present

---

## FINAL VERDICT

### Decision

- [ ] **✅ APPROVE** - All requirements met, no blockers
- [ ] **⛔ REQUEST CHANGES** - Critical blockers must be resolved
- [ ] **💬 COMMENT** - Warnings only, maintainer decision

### Summary

**Blockers**: X  
**Warnings**: Y  
**Suggestions**: Z

### Required Actions Before Merge

1. [Action 1]
2. [Action 2]
3. [Action 3]

### Optional Improvements

1. [Improvement 1]
2. [Improvement 2]

---

## Additional Comments

[Any other context, questions, or discussion points]

---

## References

- [REVIEW_GUIDELINES.md](../../../REVIEW_GUIDELINES.md)
- [Trading Best Practices](../../trading-best-practices/SKILL.md)
- [Critical Review Process](../../critical-review/SKILL.md)

---

**Reviewed By**: [Your Name]  
**Timestamp**: [ISO 8601 timestamp]
