---
name: Rust Trading Development
description: Specific rules for trading feature development
---

# Skill: Rust Trading Development

## When to use this skill

- Adding or modifying trading strategies
- Modifying the RiskManager or order validation
- Financial calculations (prices, quantities, P&L)
- Working with technical indicators

## Templates available

| Template | Usage |
|----------|-------|
| (none yet) | Add trading-specific templates as needed |

## Critical rules

### Monetary precision (MANDATORY)

```rust
// ❌ FORBIDDEN
let price: f64 = 123.45;
let total = price * quantity;

// ✅ CORRECT
use rust_decimal::Decimal;
let price = Decimal::from_str("123.45").unwrap();
let total = price * quantity;
```

**Why**: Rounding errors in `f64` can cause real financial losses.

### Risk management

Every new strategy MUST respect the flow:
```
Analyst (generates TradeProposal) 
    → RiskManager (validates)
    → Executor (executes if approved)
```

The RiskManager applies the validation chain:
1. BuyingPowerValidator
2. CircuitBreakerValidator
3. PDTValidator
4. PositionSizeValidator
5. SectorCorrelationValidator
6. SentimentValidator

### Mandatory tests

For any trading feature:
- [ ] Unit tests for each technical indicator
- [ ] Integration tests for complete flows
- [ ] Backtests on historical data (if applicable)

## Key files

| Path | Content |
|------|---------|
| `src/domain/trading/` | Trading entities (Order, Position, Trade) |
| `src/domain/risk/` | Risk management, validators |
| `src/application/strategies/` | Trading strategies |
| `src/application/analyst.rs` | Analyst agent |
| `src/application/risk_manager.rs` | RiskManager service |

## Available technical indicators

The project uses the `ta` crate for indicators:
- SMA, EMA (moving averages)
- RSI (Relative Strength Index)
- MACD (Moving Average Convergence Divergence)
- Bollinger Bands
- ADX (Average Directional Index)
- ATR (Average True Range)

## Example: Adding a new strategy

1. Create the file in `src/application/strategies/`
2. Implement the `Strategy` trait
3. Add the mode to `StrategyMode` enum
4. Register in `StrategyFactory`
5. Add tests
6. Document in `docs/STRATEGIES.md`
