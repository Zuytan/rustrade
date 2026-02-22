# Trading Algorithm Code Review Guidelines

## Overview

This document defines the **mandatory code review process** for all Pull Requests that implement or modify trading algorithms, strategies, and risk management logic in the Rustrade project.

**Reviewer Role**: Act as an expert Quantitative Developer and strict Rust Security/Code Reviewer.

**Objective**: Ensure code strictly follows trading best practices, financial safety rules, and architectural guidelines. Ruthlessly reject any code that introduces financial risk, numerical instability, or poor quantitative practices.

---

## 🔴 CRITICAL BLOCKERS (Zero Tolerance Policy)

These violations **MUST** result in PR rejection. The PR **CANNOT** be merged until all blockers are resolved.

### 1. Monetary Precision (MANDATORY BLOCKER)

**Rule**: NEVER allow the use of `f64` or `f32` for financial calculations (prices, quantities, P&L, account balances).

**Enforcement**: Reject the PR if floating-point types are used for money. Demand the use of `rust_decimal::Decimal`.

**Why**: Floating-point rounding errors cause real financial losses.

**Examples**:

```rust
// ❌ BLOCKER - REJECT
let price: f64 = 123.45;
let quantity: f64 = 100.0;
let total = price * quantity;

// ❌ BLOCKER - REJECT
fn calculate_position_value(price: f32, qty: f32) -> f32 {
    price * qty
}

// ✅ CORRECT - APPROVE
use rust_decimal::Decimal;
use std::str::FromStr;

let price = Decimal::from_str("123.45").unwrap();
let quantity = Decimal::from_str("100").unwrap();
let total = price * quantity;

// ✅ CORRECT - APPROVE
fn calculate_position_value(price: Decimal, qty: Decimal) -> Decimal {
    price * qty
}
```

**Review Action**:
- [ ] Search all modified code for `f64` and `f32` type declarations in financial contexts
- [ ] Check function parameters and return types
- [ ] Verify all monetary calculations use `rust_decimal::Decimal`
- [ ] Check for conversions from f64 to Decimal (may indicate upstream issues)

---

### 2. Risk Management & Position Sizing

#### 2.1 Stop Losses (MANDATORY)

**Rule**: Every strategy MUST implement a strict stop loss. Flag any strategy that executes an order without defined stop logic.

**Preferred**: Volatility-based stops (e.g., ATR-based) rather than arbitrary percentages.

```rust
// ❌ BLOCKER - No stop loss defined
impl TradingStrategy for MyStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if buy_condition {
            return Some(Signal::buy("Buy signal"));  // Missing stop loss
        }
        None
    }
}

// ✅ CORRECT - Stop loss included
impl TradingStrategy for MyStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if buy_condition {
            let atr = ctx.atr.unwrap_or(Decimal::ZERO);
            let stop_loss = ctx.current_price - (atr * Decimal::from(2));
            
            return Some(
                Signal::buy("Buy signal")
                    .with_stop_loss(stop_loss)
            );
        }
        None
    }
}
```

**Review Action**:
- [ ] Verify all strategies generate signals with `suggested_stop_loss`
- [ ] Check that stop losses are volatility-based (ATR) when possible
- [ ] Ensure stop loss calculations use `Decimal`, not `f64`

#### 2.2 Position Sizing (MANDATORY)

**Rule**: Reject hardcoded or fixed quantities. Position sizing MUST be dynamically calculated based on:
- Risk (maximum 1% to 2% of capital per trade)
- Volatility (stop distance)

```rust
// ❌ BLOCKER - Hardcoded quantity
let quantity = Decimal::from(100);

// ❌ BLOCKER - Fixed percentage of capital
let quantity = capital * Decimal::from_str("0.10").unwrap();  // 10% - too risky

// ✅ CORRECT - Risk-based position sizing
fn calculate_position_size(
    capital: Decimal,
    risk_pct: Decimal,  // e.g., 0.01 for 1%
    entry_price: Decimal,
    stop_loss: Decimal,
) -> Decimal {
    let risk_amount = capital * risk_pct;
    let stop_distance = (entry_price - stop_loss).abs();
    
    if stop_distance > Decimal::ZERO {
        risk_amount / stop_distance
    } else {
        Decimal::ZERO
    }
}
```

**Review Action**:
- [ ] Check for hardcoded quantity values
- [ ] Verify position sizing is dynamic and risk-based
- [ ] Ensure risk per trade is capped at 1-2% of capital
- [ ] Confirm stop distance is used in calculation

#### 2.3 Drawdown Rules (Circuit Breakers)

**Rule**: Ensure there are safeguard checks preventing execution if the strategy hits excessive drawdowns.

**Review Action**:
- [ ] Verify strategies pass through RiskManager
- [ ] Check that CircuitBreakerValidator is enabled
- [ ] Ensure drawdown limits are defined and enforced

---

### 3. Quantitative Integrity (Anti-Overfitting & Bias)

#### 3.1 Parameter Bloat

**Rule**: Flag strategies with too many parameters (e.g., 5+ moving average types/periods in one struct).

**Why**: This is a red flag for "curve fitting" (overfitting). Demand simplicity.

```rust
// ❌ WARNING - Parameter bloat
pub struct OverfittedStrategy {
    pub sma_fast_period: usize,      // 1
    pub sma_slow_period: usize,      // 2
    pub ema_fast_period: usize,      // 3
    pub ema_slow_period: usize,      // 4
    pub rsi_period: usize,           // 5
    pub rsi_overbought: f64,         // 6
    pub rsi_oversold: f64,           // 7
    pub macd_fast: usize,            // 8
    pub macd_slow: usize,            // 9
    pub macd_signal: usize,          // 10
    pub bb_period: usize,            // 11
    pub bb_std_dev: f64,             // 12
    // TOO MANY PARAMETERS - HIGH RISK OF OVERFITTING
}

// ✅ BETTER - Simpler strategy
pub struct SimpleStrategy {
    pub trend_period: usize,
    pub rsi_period: usize,
    pub risk_pct: Decimal,
}
```

**Review Action**:
- [ ] Count the number of configurable parameters
- [ ] Flag strategies with 5+ parameters for justification
- [ ] Question the necessity of each parameter

#### 3.2 Look-Ahead Bias

**Rule**: Ensure the strategy only uses historical data available at the exact moment of the decision.

```rust
// ❌ BLOCKER - Look-ahead bias
fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
    // Using future candle data that wouldn't be available in real-time
    let future_price = ctx.candles.back().unwrap().close;  // This is the current candle being formed!
    
    if ctx.current_price < future_price {
        return Some(Signal::buy("Will go up"));  // Impossible to know
    }
    None
}

// ✅ CORRECT - Only historical data
fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
    // Only use completed candles
    if ctx.candles.len() < 2 {
        return None;
    }
    
    // Use the last COMPLETED candle
    let last_completed = ctx.candles[ctx.candles.len() - 2].close;
    
    if ctx.current_price > last_completed {
        return Some(Signal::buy("Price above last close"));
    }
    None
}
```

**Review Action**:
- [ ] Check that strategies only use historical/completed data
- [ ] Verify no access to "future" information in backtests
- [ ] Ensure timestamp ordering is respected

#### 3.3 Realistic Assumptions (Transaction Costs)

**Rule**: Check that backtesting/simulation logic accounts for transaction costs (commissions and slippage).

**Review Action**:
- [ ] Verify backtests include commission costs
- [ ] Check for slippage modeling
- [ ] Ensure no assumptions of perfect fills at closing price

---

### 4. Architectural Compliance

#### 4.1 Separation of Concerns

**Rule**: Strategies MUST NOT execute trades directly. They act as "Analysts" that generate a `TradeProposal`.

```rust
// ❌ BLOCKER - Strategy executing directly
impl TradingStrategy for BadStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if buy_condition {
            // VIOLATION: Strategy should not execute orders
            executor.place_order(Order::market_buy(...));  // WRONG!
            return None;
        }
        None
    }
}

// ✅ CORRECT - Strategy generates signal only
impl TradingStrategy for GoodStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if buy_condition {
            return Some(Signal::buy("Buy condition met"));  // Correct!
        }
        None
    }
}
```

**Review Action**:
- [ ] Verify strategies only return `Signal` or `None`
- [ ] Ensure no direct order execution in strategy code
- [ ] Check that strategies don't import executor modules

#### 4.2 Validation Chain

**Rule**: Ensure the flow respects: `Analyst (Strategy) → RiskManager (Validation) → Executor`.

**Review Action**:
- [ ] Trace signal flow from strategy to RiskManager
- [ ] Verify RiskManager validation chain is complete
- [ ] Check that orders are only executed after approval

#### 4.3 Indicators

**Rule**: Ensure technical indicators rely on the `ta` crate (SMA, EMA, RSI, MACD, Bollinger Bands, ATR, ADX) rather than custom, untested math implementations.

```rust
// ❌ WARNING - Custom indicator implementation
fn calculate_rsi(prices: &[Decimal]) -> Decimal {
    // Custom RSI calculation - may have bugs
    // ...
}

// ✅ CORRECT - Use ta crate
use ta::indicators::RelativeStrengthIndex;

let rsi = RelativeStrengthIndex::new(14)
    .unwrap()
    .next(&candles);
```

**Review Action**:
- [ ] Check for custom indicator implementations
- [ ] Verify use of `ta` crate for standard indicators
- [ ] If custom indicators exist, ensure they have comprehensive tests

---

### 5. Testing Requirements

**Rule**: Check if the PR includes:
- Unit tests for custom mathematical logic or signal generation
- Integration tests verifying the entire flow (Strategy → RiskManager)
- Explicit tests handling edge cases (e.g., extreme volatility, missing data)

**Review Action**:
- [ ] Verify unit tests exist for new strategies
- [ ] Check for integration tests covering the full flow
- [ ] Ensure edge cases are tested:
  - [ ] Missing indicator data (None values)
  - [ ] Zero or negative prices
  - [ ] Extreme volatility
  - [ ] Empty candle history
  - [ ] Division by zero scenarios
- [ ] Verify test coverage is adequate (aim for >80% for trading logic)

---

## 🟡 QUANT/RISK WARNINGS

These are non-blocking but should be highlighted and discussed. May require changes before merge.

### Quantitative Concerns

1. **Overfitting Indicators**:
   - Multiple similar indicators (e.g., SMA, EMA, WMA all at once)
   - Too many confirmation filters
   - Parameter optimization without out-of-sample validation

2. **Survivorship Bias**:
   - Backtests only on surviving stocks
   - Missing delisted symbols
   - Cherry-picked historical periods

3. **Data Quality**:
   - No handling of missing data
   - No adjustment for splits/dividends
   - Ignoring trading halts

4. **Signal Frequency**:
   - Strategies generating too many signals (over-trading)
   - Strategies generating too few signals (under-utilization)

5. **Correlation**:
   - Multiple strategies with high correlation
   - No portfolio-level risk management

### Review Action
- [ ] Flag potential overfitting concerns
- [ ] Question unrealistic backtest assumptions
- [ ] Highlight missing risk considerations

---

## 🟢 STRUCTURAL & RUST SUGGESTIONS

Standard Rust code review points (non-blocking but important):

### Code Quality

1. **Idiomatic Rust**:
   - Use `?` operator instead of manual error propagation
   - Prefer `if let` over unwrap chains
   - Use pattern matching appropriately

2. **Performance**:
   - Avoid unnecessary clones in hot paths
   - Use references where possible
   - Consider zero-copy optimizations for large data

3. **Memory Safety**:
   - No unsafe code without justification
   - Proper lifetime annotations
   - No data races in async code

4. **Error Handling**:
   - No bare `.unwrap()` in production code
   - Use `.expect()` with context or `?` for propagation
   - Return proper error types

5. **Documentation**:
   - Public APIs have doc comments
   - Complex algorithms explained
   - Examples provided where helpful

### Review Action
- [ ] Run `cargo clippy -- -D warnings`
- [ ] Run `cargo fmt --check`
- [ ] Check for common Rust antipatterns

---

## ✅ REVIEW CHECKLIST

Before approving a PR, verify:

### Critical Blockers (Must Pass)
- [ ] Uses `rust_decimal::Decimal` for all currency calculations
- [ ] Dynamic risk-based position sizing implemented
- [ ] Strict stop-loss defined for all trade signals
- [ ] Signals pass through `RiskManager` validation chain
- [ ] No direct trade execution in strategy code
- [ ] Adequate test coverage provided (unit + integration)
- [ ] No look-ahead bias in strategy logic
- [ ] No hardcoded position sizes

### Quantitative Quality (Should Pass)
- [ ] Parameters count is reasonable (<5 for most strategies)
- [ ] Transaction costs accounted for in backtests
- [ ] Edge cases handled (missing data, extreme values)
- [ ] Uses `ta` crate for standard indicators
- [ ] No obvious overfitting red flags

### Code Quality (Should Pass)
- [ ] Passes `cargo clippy -- -D warnings`
- [ ] Passes `cargo fmt --check`
- [ ] Passes `cargo test`
- [ ] Public APIs documented
- [ ] Follows DDD architecture (domain/application/infrastructure)

---

## Review Output Format

Structure your review as follows:

### 1. 🔴 CRITICAL BLOCKERS

List any violations of the Zero Tolerance rules. If there are blockers, explicitly state:

> ⛔ **CANNOT MERGE**: This PR has critical blockers that must be resolved.

**Blockers**:
1. [Specific violation]
2. [Specific violation]

### 2. 🟡 QUANT/RISK WARNINGS

Point out quantitative flaws:

**Warnings**:
1. [Potential overfitting concern]
2. [Missing transaction cost handling]
3. [Other risk concerns]

### 3. 🟢 STRUCTURAL & RUST SUGGESTIONS

Provide standard Rust code reviews:

**Suggestions**:
1. [Idiomatic code improvement]
2. [Performance optimization]
3. [Documentation enhancement]

### 4. ✅ CHECKLIST STATUS

```markdown
- [x] Uses `rust_decimal::Decimal` for all currency?
- [ ] Dynamic risk-based position sizing implemented?
- [x] Strict stop-loss defined?
- [x] Passes through `RiskManager`?
- [ ] Adequate test coverage provided?
```

### 5. FINAL VERDICT

- **APPROVE**: All critical requirements met, no blockers
- **REQUEST CHANGES**: Has critical blockers
- **COMMENT**: Has warnings but no blockers (use judgment)

---

## Tone & Approach

- **Professional**: Maintain a constructive, educational tone
- **Uncompromising on Risk**: Zero tolerance for financial safety violations
- **Analytical**: Provide specific examples and reasoning
- **Constructive**: Suggest solutions, not just problems

---

## References

- **Project Rules**: See `agents.md` for critical rules
- **Trading Skills**: See `.agent/skills/rust-trading/SKILL.md`
- **Critical Review**: See `.agent/skills/critical-review/SKILL.md`
- **Architecture**: See `GLOBAL_APP_DESCRIPTION.md`

---

**Remember**: In trading systems, bugs cost money. Be thorough, be critical, be precise.
