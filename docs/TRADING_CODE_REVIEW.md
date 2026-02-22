# Trading Code Review System

This directory contains a comprehensive code review system for trading algorithms in the Rustrade project. The system enforces strict financial safety rules, quantitative best practices, and architectural compliance.

## Overview

The review system consists of:

1. **REVIEW_GUIDELINES.md** - Complete review requirements and examples
2. **GitHub Actions** - Automated PR checks (`.github/workflows/trading-review.yml`)
3. **Review Script** - Manual review tool (`scripts/review_trading_code.sh`)
4. **Clippy Configuration** - Custom lint rules (`.cargo/clippy.toml`)
5. **PR Templates** - Structured checklists for contributors
6. **Agent Skills** - Review guidance for AI agents (`.agent/skills/trading-code-review/`)

## Quick Start

### For Contributors

Before submitting a PR that modifies trading code:

```bash
# 1. Run the automated review script
./scripts/review_trading_code.sh

# 2. Fix any blockers or warnings
# 3. Run tests
cargo test

# 4. Submit PR using the trading_strategy template
```

### For Reviewers

When reviewing a trading-related PR:

```bash
# 1. Run the automated review
./scripts/review_trading_code.sh

# 2. Review the REVIEW_GUIDELINES.md checklist
# 3. Use templates in .agent/skills/trading-code-review/templates/
# 4. Approve only if all critical requirements are met
```

## Critical Rules (Zero Tolerance)

### 1. No Float Types for Money
```rust
// ❌ BLOCKER
let price: f64 = 123.45;

// ✅ CORRECT
use rust_decimal::Decimal;
let price = Decimal::from_str("123.45").unwrap();
```

### 2. Stop Losses Required
```rust
// ❌ BLOCKER
Signal::buy("reason")

// ✅ CORRECT
Signal::buy("reason")
    .with_stop_loss(entry_price - atr * Decimal::from(2))
```

### 3. Dynamic Position Sizing
```rust
// ❌ BLOCKER
let quantity = Decimal::from(100);

// ✅ CORRECT
let risk_amount = capital * risk_pct;
let quantity = risk_amount / stop_distance;
```

### 4. No Direct Execution
```rust
// ❌ BLOCKER - Strategy executes orders
impl TradingStrategy for BadStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        executor.place_order(...);  // WRONG!
    }
}

// ✅ CORRECT - Strategy returns signal
impl TradingStrategy for GoodStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        Some(Signal::buy("reason"))  // Correct!
    }
}
```

## Review Process

### Automated Checks

The GitHub Action automatically checks:
- Float type usage in financial code
- Direct order execution in strategies
- Missing stop losses
- Hardcoded quantities
- .unwrap() usage
- Test coverage

### Manual Review

Reviewers must verify:
- Quantitative integrity (no overfitting)
- No look-ahead bias
- Transaction costs in backtests
- Edge cases tested
- Risk parameters reasonable

## Files and Directories

```
.
├── REVIEW_GUIDELINES.md              # Complete review requirements
├── .github/
│   ├── workflows/
│   │   └── trading-review.yml        # Automated PR checks
│   ├── pull_request_template.md      # Default PR template
│   └── PULL_REQUEST_TEMPLATE/
│       └── trading_strategy.md       # Trading-specific PR template
├── .cargo/
│   └── clippy.toml                   # Custom lint rules
├── scripts/
│   └── review_trading_code.sh        # Manual review script
├── .agent/skills/trading-code-review/
│   ├── SKILL.md                      # Review skill guide
│   └── templates/
│       ├── quick_review.md           # Quick checklist
│       └── detailed_review.md        # Detailed report template
└── tests/
    └── test_review_violations.rs     # Test file with violations
```

## Examples

### Running the Review Script

```bash
# Check all strategies
./scripts/review_trading_code.sh

# Check specific path
./scripts/review_trading_code.sh src/application/strategies/my_strategy.rs

# Check multiple paths
./scripts/review_trading_code.sh src/application/strategies/ src/domain/trading/
```

### Expected Output

```
╔══════════════════════════════════════════════════════════════╗
║          RUSTRADE TRADING CODE REVIEW SCRIPT                 ║
╚══════════════════════════════════════════════════════════════╝

Checking path: src/application/strategies/

═══ 1. MONETARY PRECISION CHECK (CRITICAL) ═══
✅ PASSED: No f64/f32 usage in financial code

═══ 2. ERROR HANDLING CHECK ═══
✅ PASSED: No .unwrap() in production code

...

═══════════════════════════════════════════════════════════════
                      REVIEW SUMMARY                            
═══════════════════════════════════════════════════════════════

⛔ Blockers: 0
🟡 Warnings: 0
✅ Passed: 8

╔══════════════════════════════════════════════════════════════╗
║  ✅ ALL CHECKS PASSED - Ready for manual review             ║
╚══════════════════════════════════════════════════════════════╝
```

## Integration with CI/CD

The `trading-review.yml` GitHub Action runs automatically on PRs that modify:
- `src/application/strategies/**`
- `src/application/risk_management/**`
- `src/domain/trading/**`
- `src/domain/risk/**`

Results appear in:
- PR checks status
- GitHub Actions summary
- PR comments (if configured)

## Best Practices

1. **Review Early**: Run the script during development, not just before PR
2. **Fix Blockers First**: Address critical issues before warnings
3. **Understand Why**: Don't just fix violations, understand the financial risk
4. **Add Tests**: Every fix should include tests to prevent regression
5. **Document Decisions**: If you disagree with a rule, document why

## Common Issues

### False Positives

The automated checks may flag:
- Test code using f64 (acceptable in test helpers)
- Confidence scores using f64 (not monetary, acceptable)
- Mock data generation

**Resolution**: Reviewers should use judgment. Automated checks are conservative.

### Overfitting Warnings

If your strategy has many parameters:
1. Justify each parameter's necessity
2. Provide out-of-sample validation
3. Consider simplifying the strategy

### Missing Tests

Every trading strategy must have:
- Unit tests for signal generation
- Integration tests for full flow
- Edge case tests (None values, extremes)

## Contributing

To improve the review system:

1. Update `REVIEW_GUIDELINES.md` with new requirements
2. Enhance `scripts/review_trading_code.sh` with new checks
3. Add more examples to documentation
4. Create additional test cases

## Resources

- [REVIEW_GUIDELINES.md](../REVIEW_GUIDELINES.md) - Complete requirements
- [CONTRIBUTING.md](../CONTRIBUTING.md) - General contribution guide
- [agents.md](../agents.md) - Agent protocol and skills
- [Trading Best Practices](.agent/skills/trading-best-practices/SKILL.md)

## Support

Questions about the review process:
- Open an issue with the `question` label
- Discuss in PR comments
- Refer to REVIEW_GUIDELINES.md

## License

Same as the main project (MIT License).

---

**Remember**: In trading systems, bugs cost money. Be thorough, be critical, be precise.
