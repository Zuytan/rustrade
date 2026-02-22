# Example Violations for Testing Review System

This document provides example code with intentional violations to test the review system.

## Example 1: Float Type Violation

```rust
// ❌ BLOCKER: Using f64 for financial calculations
pub struct BadStrategy {
    pub price_threshold: f64,  // VIOLATION: Should use Decimal
}

impl TradingStrategy for BadStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let price = ctx.current_price.to_string().parse::<f64>().unwrap();
        
        if price > self.price_threshold {
            return Some(Signal::buy("Price above threshold"));
        }
        None
    }
    
    fn name(&self) -> &str {
        "BadStrategy"
    }
}
```

**Expected Review Feedback**:
- ⛔ BLOCKER: Float type (f64) used for monetary value
- ⛔ BLOCKER: No stop loss defined in signal
- ⛔ BLOCKER: Uses .unwrap() without context

**Correct Version**:

```rust
// ✅ CORRECT: Using Decimal for financial calculations
use rust_decimal::Decimal;

pub struct GoodStrategy {
    pub price_threshold: Decimal,  // Correct: Uses Decimal
}

impl TradingStrategy for GoodStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if ctx.current_price > self.price_threshold {
            let atr = ctx.atr.unwrap_or(Decimal::ZERO);
            let stop_loss = ctx.current_price - (atr * Decimal::from(2));
            
            return Some(
                Signal::buy("Price above threshold")
                    .with_stop_loss(stop_loss)  // Correct: Stop loss defined
            );
        }
        None
    }
    
    fn name(&self) -> &str {
        "GoodStrategy"
    }
}
```

## Example 2: Missing Stop Loss

```rust
// ❌ BLOCKER: No stop loss
impl TradingStrategy for NoStopLossStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if ctx.rsi? < Decimal::from(30) {
            return Some(Signal::buy("RSI oversold"));  // VIOLATION: No stop loss
        }
        None
    }
    
    fn name(&self) -> &str {
        "NoStopLossStrategy"
    }
}
```

**Expected Review Feedback**:
- ⛔ BLOCKER: Signal created without stop loss

**Correct Version**:

```rust
// ✅ CORRECT: Stop loss included
impl TradingStrategy for WithStopLossStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if let Some(rsi) = ctx.rsi {
            if rsi < Decimal::from(30) {
                let atr = ctx.atr.unwrap_or(Decimal::ZERO);
                let stop_loss = ctx.current_price - (atr * Decimal::from(2));
                
                return Some(
                    Signal::buy("RSI oversold")
                        .with_stop_loss(stop_loss)  // Correct!
                );
            }
        }
        None
    }
    
    fn name(&self) -> &str {
        "WithStopLossStrategy"
    }
}
```

## Example 3: Hardcoded Position Size

```rust
// ❌ BLOCKER: Hardcoded quantity
fn calculate_order_size(price: Decimal) -> Decimal {
    Decimal::from(100)  // VIOLATION: Hardcoded quantity
}
```

**Expected Review Feedback**:
- ⛔ BLOCKER: Position size is hardcoded, not risk-based

**Correct Version**:

```rust
// ✅ CORRECT: Risk-based position sizing
fn calculate_order_size(
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

## Example 4: Direct Order Execution

```rust
// ❌ BLOCKER: Strategy executing orders directly
pub struct DirectExecutionStrategy {
    executor: Arc<dyn OrderExecutor>,
}

impl TradingStrategy for DirectExecutionStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if buy_condition {
            // VIOLATION: Strategy should not execute orders
            self.executor.place_order(Order::market_buy(...));
            return None;
        }
        None
    }
    
    fn name(&self) -> &str {
        "DirectExecutionStrategy"
    }
}
```

**Expected Review Feedback**:
- ⛔ BLOCKER: Strategy executes orders directly, violating separation of concerns
- ⛔ BLOCKER: Bypasses RiskManager validation

**Correct Version**:

```rust
// ✅ CORRECT: Strategy only returns signal
pub struct ProperStrategy;

impl TradingStrategy for ProperStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if buy_condition {
            let stop_loss = calculate_stop(ctx);
            
            // Correct: Only return signal, let RiskManager and Executor handle execution
            return Some(
                Signal::buy("Buy condition met")
                    .with_stop_loss(stop_loss)
            );
        }
        None
    }
    
    fn name(&self) -> &str {
        "ProperStrategy"
    }
}
```

## Example 5: Parameter Bloat (Overfitting)

```rust
// 🟡 WARNING: Too many parameters
pub struct OverfittedStrategy {
    pub sma_fast_period: usize,      // 1
    pub sma_slow_period: usize,      // 2
    pub ema_fast_period: usize,      // 3
    pub ema_slow_period: usize,      // 4
    pub rsi_period: usize,           // 5
    pub rsi_overbought: Decimal,     // 6
    pub rsi_oversold: Decimal,       // 7
    pub macd_fast: usize,            // 8
    pub macd_slow: usize,            // 9
    pub macd_signal: usize,          // 10
    pub bb_period: usize,            // 11
    pub bb_std_dev: Decimal,         // 12
    pub adx_threshold: Decimal,      // 13
    // 13 parameters - HIGH RISK OF OVERFITTING
}
```

**Expected Review Feedback**:
- 🟡 WARNING: 13 configurable parameters detected
- 🟡 WARNING: High risk of curve fitting to historical data
- 🟡 WARNING: Unlikely to generalize to live trading

**Correct Version**:

```rust
// ✅ BETTER: Simplified strategy
pub struct SimpleStrategy {
    pub trend_period: usize,      // 1
    pub momentum_period: usize,   // 2
    pub risk_pct: Decimal,        // 3
    // Only 3 essential parameters
}
```

## Example 6: Look-Ahead Bias

```rust
// 🟡 WARNING: Look-ahead bias
impl TradingStrategy for LookAheadStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // VIOLATION: Using the current (incomplete) candle
        let current_candle = ctx.candles.back().unwrap();
        
        if ctx.current_price < current_candle.high {
            // This uses future information not available in real-time!
            return Some(Signal::buy("Will go up"));
        }
        None
    }
    
    fn name(&self) -> &str {
        "LookAheadStrategy"
    }
}
```

**Expected Review Feedback**:
- 🟡 WARNING: Potential look-ahead bias - using incomplete candle data
- 🟡 WARNING: Strategy assumes knowledge of future price movement

**Correct Version**:

```rust
// ✅ CORRECT: Only uses completed historical data
impl TradingStrategy for NoLookAheadStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if ctx.candles.len() < 2 {
            return None;
        }
        
        // Use only completed candles (not the current forming one)
        let last_completed = &ctx.candles[ctx.candles.len() - 2];
        
        if ctx.current_price > last_completed.high {
            let stop_loss = last_completed.low;
            return Some(
                Signal::buy("Breakout above previous high")
                    .with_stop_loss(stop_loss)
            );
        }
        None
    }
    
    fn name(&self) -> &str {
        "NoLookAheadStrategy"
    }
}
```

## Testing the Review System

To test the review system with these examples:

1. **Create a branch with violations**:
   ```bash
   git checkout -b test/review-violations
   ```

2. **Add code with violations** (use examples above)

3. **Run the review script**:
   ```bash
   ./scripts/review_trading_code.sh
   ```

4. **Verify it catches violations**:
   - Should see ⛔ BLOCKER messages for critical issues
   - Should see 🟡 WARNING messages for concerns
   - Script should exit with code 1 if blockers found

5. **Fix violations and re-run**:
   - Replace f64 with Decimal
   - Add stop losses
   - Use risk-based position sizing
   - Remove direct execution
   - Script should pass with code 0

## Automated Testing

The GitHub Action will automatically check PRs. To trigger it locally:

1. Push a branch with trading code changes
2. Open a PR
3. Check the "Trading Code Review" action in the PR checks
4. Review the summary for detected violations

## Summary

These examples demonstrate:
- ⛔ **Critical blockers** that prevent merge
- 🟡 **Warnings** that require justification
- ✅ **Correct patterns** to follow

Use these examples to:
- Train new developers
- Test the review system
- Validate automated checks
- Document best practices
