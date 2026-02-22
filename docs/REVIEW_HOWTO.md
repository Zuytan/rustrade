# How to Use the Trading Code Review System

This guide provides step-by-step instructions for using the Rustrade trading code review system.

## For Contributors

### Before You Start

When working on trading code (strategies, risk management, financial calculations), you must follow strict safety rules. This guide will help you pass the review process on your first try.

### Step-by-Step Process

#### 1. Understand the Critical Rules

Before writing code, familiarize yourself with the zero-tolerance rules:

```bash
# Read the guidelines (5-10 minutes)
cat REVIEW_GUIDELINES.md | less

# Or view in browser
# Open REVIEW_GUIDELINES.md in GitHub
```

**Key Rules to Remember**:
- ✅ Use `Decimal` for money, NOT `f64`
- ✅ Always set stop losses on signals
- ✅ Calculate position size dynamically based on risk
- ✅ Strategies only return `Signal`, never execute orders

#### 2. Write Your Code

Follow the correct patterns from `docs/REVIEW_EXAMPLES.md`:

```rust
use rust_decimal::Decimal;
use crate::application::strategies::traits::{Signal, TradingStrategy, AnalysisContext};

pub struct MyStrategy {
    pub risk_pct: Decimal,  // ✅ Use Decimal
}

impl TradingStrategy for MyStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if buy_condition {
            // ✅ Calculate stop loss
            let atr = ctx.atr.unwrap_or(Decimal::ZERO);
            let stop_loss = ctx.current_price - (atr * Decimal::from(2));
            
            // ✅ Return signal with stop loss
            return Some(
                Signal::buy("Buy condition met")
                    .with_stop_loss(stop_loss)
            );
        }
        None
    }
    
    fn name(&self) -> &str {
        "MyStrategy"
    }
}
```

#### 3. Write Tests

Every strategy needs tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generates_signal_with_stop_loss() {
        let strategy = MyStrategy { risk_pct: Decimal::from_str("0.01").unwrap() };
        let ctx = create_test_context();
        
        let signal = strategy.analyze(&ctx);
        
        assert!(signal.is_some());
        assert!(signal.unwrap().suggested_stop_loss.is_some());
    }
    
    #[test]
    fn test_handles_missing_atr() {
        // Test edge cases
    }
}
```

#### 4. Run the Review Script

Before committing, run the automated review:

```bash
# Run review on your changes
./scripts/review_trading_code.sh

# If you modified specific files:
./scripts/review_trading_code.sh src/application/strategies/my_strategy.rs
```

**Expected Output**:

```
╔══════════════════════════════════════════════════════════════╗
║          RUSTRADE TRADING CODE REVIEW SCRIPT                 ║
╚══════════════════════════════════════════════════════════════╝

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
```

#### 5. Fix Any Issues

If blockers are found:

```bash
# Example: Fix f64 usage
# ❌ Before
let price: f64 = 123.45;

# ✅ After
use rust_decimal::Decimal;
let price = Decimal::from_str("123.45").unwrap();

# Run review again
./scripts/review_trading_code.sh
```

#### 6. Run Standard Checks

```bash
# Format code
cargo fmt

# Run linter (must pass with no warnings)
cargo clippy --all-targets -- -D warnings

# Run tests
cargo test
```

#### 7. Commit and Push

```bash
git add .
git commit -m "feat: add MyStrategy with proper risk management"
git push origin feature/my-strategy
```

#### 8. Create Pull Request

1. Go to GitHub and create a PR
2. **Use the trading strategy template** if modifying trading code
3. Fill out the checklist completely
4. Wait for automated checks to complete

#### 9. Address Review Feedback

If reviewers request changes:

1. Read the feedback carefully
2. Make the requested changes
3. Run `./scripts/review_trading_code.sh` again
4. Push the changes
5. Respond to comments explaining what you changed

## For Reviewers

### Quick Review (5-10 minutes)

For simple PRs with small changes:

#### 1. Run Automated Checks

```bash
# Check out the PR branch
gh pr checkout 123

# Run review script
./scripts/review_trading_code.sh
```

#### 2. Use Quick Checklist

Copy `templates/quick_review.md` and fill it out:

```markdown
## ⛔ CRITICAL BLOCKERS

- [x] NO f64/f32 for money
- [x] NO direct order execution
- [x] Stop losses defined
- [x] Dynamic position sizing

**Blockers Found**: 0

✅ APPROVE
```

#### 3. Post Review

If all checks pass:
- Approve the PR
- Add comment: "✅ Trading code review passed. All financial safety requirements met."

If blockers found:
- Request changes
- List specific violations
- Reference REVIEW_GUIDELINES.md sections

### Detailed Review (20-30 minutes)

For complex PRs with new strategies:

#### 1. Run All Checks

```bash
# Automated review
./scripts/review_trading_code.sh

# Check compilation
cargo check

# Run tests
cargo test

# Run clippy
cargo clippy --all-targets -- -D warnings
```

#### 2. Code Analysis

Review the code manually checking:

**Monetary Precision**:
```bash
# Search for float usage
grep -r "f64\|f32" src/application/strategies/new_strategy.rs
```

**Risk Management**:
- Every signal has stop loss? ✓
- Position sizing dynamic? ✓
- Risk percentage reasonable (1-2%)? ✓

**Quantitative Quality**:
- Parameter count < 5? ✓
- No look-ahead bias? ✓
- Uses `ta` crate for indicators? ✓

**Architecture**:
- Strategy only returns Signal? ✓
- No direct executor calls? ✓
- Follows DDD structure? ✓

**Testing**:
- Unit tests present? ✓
- Edge cases covered? ✓
- Tests would catch regressions? ✓

#### 3. Use Detailed Template

Copy `.agent/skills/trading-code-review/templates/detailed_review.md` and fill it out completely.

Include:
- Blockers (if any)
- Warnings (concerns)
- Suggestions (improvements)
- Checklist status
- Final verdict

#### 4. Submit Review

Post your completed review as a PR comment.

If blockers: **Request Changes**
If warnings only: **Comment** (maintainer decision)
If all pass: **Approve**

## Common Scenarios

### Scenario 1: f64 Usage Detected

**Problem**: Script reports f64 usage

**Solution**:
```bash
# Check if it's in production or test code
grep -B5 -A5 "f64" src/application/strategies/my_strategy.rs

# If in production code → BLOCKER, must fix
# If in test helper (mock_candle) → OK, acceptable
# If for confidence score → OK, not monetary
```

### Scenario 2: Missing Stop Loss

**Problem**: Signal created without stop loss

**Solution**:
```rust
// Add stop loss calculation
let atr = ctx.atr.unwrap_or(Decimal::ZERO);
let stop_loss = ctx.current_price - (atr * Decimal::from(2));

// Add to signal
Signal::buy("reason")
    .with_stop_loss(stop_loss)
```

### Scenario 3: Many Parameters

**Problem**: Strategy has 8 configurable parameters

**Questions to Ask**:
1. Are all parameters necessary?
2. Can some be combined or removed?
3. Is there out-of-sample validation?
4. Is there a risk of overfitting?

**Action**: Request justification or simplification

### Scenario 4: Test Failures

**Problem**: Tests fail after changes

**Solution**:
```bash
# Run specific test
cargo test test_name -- --nocapture

# Check what changed
git diff main src/application/strategies/

# Fix the test or code
# Re-run review
./scripts/review_trading_code.sh
```

## Tips and Best Practices

### For Contributors

1. **Review examples first**: Read `docs/REVIEW_EXAMPLES.md` before coding
2. **Run script often**: Don't wait until PR time
3. **Write tests first**: TDD helps catch issues early
4. **Ask questions**: If unsure, ask in issue/PR comments
5. **Use Decimal everywhere**: Better safe than sorry

### For Reviewers

1. **Be thorough**: Trading bugs cost money
2. **Be constructive**: Explain why, not just what
3. **Be consistent**: Apply rules uniformly
4. **Be educational**: Help contributors learn
5. **Use templates**: Structured reviews are clearer

## Troubleshooting

### Script Won't Run

```bash
# Make executable
chmod +x scripts/review_trading_code.sh

# Check bash available
which bash

# Run directly
bash scripts/review_trading_code.sh
```

### False Positives

If script flags acceptable code:
- Document in PR why it's OK
- Reviewers will use judgment
- Consider improving script later

### Clippy Warnings

```bash
# See full clippy output
cargo clippy --all-targets

# Fix specific warning
cargo clippy --fix --allow-dirty

# Re-run
cargo clippy -- -D warnings
```

## Getting Help

If you need help:

1. **Read docs**:
   - REVIEW_GUIDELINES.md
   - docs/REVIEW_EXAMPLES.md
   - docs/TRADING_CODE_REVIEW.md

2. **Ask in PR**: Tag maintainers with specific questions

3. **Open issue**: For bugs in review system

4. **Check CI logs**: GitHub Actions output has details

## Summary Checklist

Before submitting your PR, verify:

- [ ] Read REVIEW_GUIDELINES.md
- [ ] Followed correct patterns from REVIEW_EXAMPLES.md
- [ ] Ran `./scripts/review_trading_code.sh` - no blockers
- [ ] Ran `cargo fmt` - formatted
- [ ] Ran `cargo clippy -- -D warnings` - no warnings
- [ ] Ran `cargo test` - all pass
- [ ] Wrote tests for new code
- [ ] Used trading PR template
- [ ] Filled out complete checklist

If all checked: **Ready to submit!** ✅

---

**Remember**: The review process protects everyone from financial bugs. Follow it carefully, and your PR will merge smoothly.
