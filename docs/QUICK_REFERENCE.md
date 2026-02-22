# Trading Code Review - Quick Reference Card

## 🚨 Critical Rules (BLOCKERS)

| Rule | ❌ Wrong | ✅ Correct |
|------|---------|-----------|
| **Monetary Precision** | `let price: f64 = 123.45;` | `let price = Decimal::from_str("123.45").unwrap();` |
| **Stop Loss** | `Signal::buy("reason")` | `Signal::buy("reason").with_stop_loss(stop)` |
| **Position Sizing** | `Decimal::from(100)` | `risk_amount / stop_distance` |
| **Separation** | `executor.place_order(...)` | `return Some(Signal::buy(...))` |

## 📋 Quick Checklist

Before submitting your PR:

- [ ] Uses `Decimal` for ALL money calculations
- [ ] Every signal has `.with_stop_loss()`
- [ ] Position sizing is dynamic and risk-based
- [ ] Strategy only returns `Signal`, never executes
- [ ] Tests included (unit + integration + edge cases)
- [ ] Ran `./scripts/review_trading_code.sh` - zero blockers
- [ ] Ran `cargo fmt` and `cargo clippy -- -D warnings`
- [ ] Ran `cargo test` - all pass

## 🔧 Quick Commands

```bash
# Run review
./scripts/review_trading_code.sh

# Format code
cargo fmt

# Lint (must pass with no warnings)
cargo clippy --all-targets -- -D warnings

# Test
cargo test

# All checks
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

## 📚 Documentation

| Document | Purpose | Time |
|----------|---------|------|
| [REVIEW_GUIDELINES.md](REVIEW_GUIDELINES.md) | Complete requirements | 30 min |
| [docs/REVIEW_HOWTO.md](docs/REVIEW_HOWTO.md) | Step-by-step guide | 10 min |
| [docs/REVIEW_EXAMPLES.md](docs/REVIEW_EXAMPLES.md) | Violation examples | 15 min |

## 🎯 Common Patterns

### Correct Stop Loss Implementation
```rust
let atr = ctx.atr.unwrap_or(Decimal::ZERO);
let stop_loss = ctx.current_price - (atr * Decimal::from(2));

Signal::buy("reason").with_stop_loss(stop_loss)
```

### Correct Position Sizing
```rust
fn calculate_position_size(
    capital: Decimal,
    risk_pct: Decimal,     // 0.01 = 1%
    entry: Decimal,
    stop: Decimal,
) -> Decimal {
    let risk_amount = capital * risk_pct;
    let stop_distance = (entry - stop).abs();
    risk_amount / stop_distance
}
```

### Correct Decimal Usage
```rust
use rust_decimal::Decimal;
use std::str::FromStr;

let price = Decimal::from_str("123.45").unwrap();
let quantity = Decimal::from(100);
let total = price * quantity;
```

## 🚫 Common Mistakes

1. **Using f64 for prices** → Use `Decimal`
2. **Forgetting stop loss** → Use `.with_stop_loss()`
3. **Hardcoded quantities** → Calculate based on risk
4. **Strategy executes orders** → Return `Signal` only
5. **Using .unwrap() everywhere** → Use `?` or `.expect()` with context
6. **Too many parameters (>5)** → Simplify to avoid overfitting

## 🔍 Review Script Output

```
✅ Passed: 8    → Ready to submit
🟡 Warnings: 2  → Review carefully
⛔ Blockers: 1  → MUST fix before merge
```

## 📞 Help

- **Questions**: Open issue with `question` label
- **Bugs in review system**: Open issue with `review-system` label
- **Unclear requirements**: Comment on PR, tag maintainers

## 🎓 Learning Path

1. **Read** REVIEW_GUIDELINES.md (30 min)
2. **Study** docs/REVIEW_EXAMPLES.md (15 min)
3. **Practice** Fix example violations (30 min)
4. **Apply** Write your strategy (varies)
5. **Verify** Run review script (2 min)

## ⚡ Speed Tips

- Keep this card open while coding
- Use correct patterns from examples
- Run review script frequently
- Fix blockers immediately
- Ask questions early

---

**Remember**: Zero tolerance for financial bugs. When in doubt, ask!

Print this card or keep it open while developing trading code.
