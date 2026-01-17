---
name: Benchmarking & Performance
description: Trading performance evaluation via backtesting and metrics
---

# Skill: Benchmarking & Performance

## When to use this skill

- After adding or modifying a strategy
- To validate that a strategy is profitable
- To compare different configurations
- Before going from paper trading to live

## Available scripts

| Script | Usage |
|--------|-------|
| `scripts/quick_benchmark.sh SYMBOL [DAYS]` | Quick benchmark |
| `scripts/validate_strategy.sh STRATEGY` | Multi-period validation |

## Key metrics to monitor

### Profitability metrics

| Metric | Description | Acceptable threshold |
|--------|-------------|---------------------|
| **Total Return** | Total return over period | > 0% |
| **Win Rate** | % of winning trades | > 50% (trend) or > 40% (mean rev) |
| **Profit Factor** | Gains / Losses | > 1.5 |
| **Average Trade** | Average P&L per trade | > 0 |

### Risk metrics

| Metric | Description | Acceptable threshold |
|--------|-------------|---------------------|
| **Sharpe Ratio** | Risk-adjusted return | > 1.0 (good), > 2.0 (excellent) |
| **Sortino Ratio** | Same but penalizes downside | > 1.5 |
| **Max Drawdown** | Maximum loss from peak | < 20% |
| **Time in Market** | % of time with position | Depends on strategy |

### Interpretation

```
Sharpe Ratio:
  < 0.5  → Bad, don't use
  0.5-1  → Mediocre, needs improvement
  1-2    → Good
  2-3    → Very good
  > 3    → Excellent (or suspicious, check overfitting)

Max Drawdown:
  < 10%  → Conservative
  10-20% → Moderate
  20-30% → Aggressive
  > 30%  → Dangerous
```

## Benchmark commands

### Simple benchmark

```bash
# Backtest on one symbol
cargo run --bin benchmark -- --symbol AAPL --days 365

# Backtest on multiple symbols
cargo run --bin benchmark -- --symbols "AAPL,GOOGL,MSFT" --days 365
```

### Advanced benchmark

```bash
# Parallel mode (multi-core)
cargo run --bin benchmark -- --parallel --symbols "AAPL,GOOGL,MSFT"

# With sequential comparison
cargo run --bin benchmark -- --compare-sequential

# Parameter matrix
cargo run --bin benchmark_matrix
```

### Available scripts

```bash
# Stock benchmark
./scripts/benchmark_stocks.sh

# Market regime benchmark
./scripts/run_regime_benchmarks.sh

# Automatic benchmark
./scripts/auto_benchmark.sh
```

## Strategy validation workflow

### Step 1: Initial backtest

```bash
cargo run --bin benchmark -- --strategy <STRATEGY> --days 365
```

Verify:
- [ ] Sharpe Ratio > 1.0
- [ ] Max Drawdown < 20%
- [ ] Win Rate consistent with strategy type
- [ ] Profit Factor > 1.5

### Step 2: Test on different periods

```bash
# Bull period
cargo run --bin benchmark -- --start 2021-01-01 --end 2021-12-31

# Bear period
cargo run --bin benchmark -- --start 2022-01-01 --end 2022-12-31

# Volatile period
cargo run --bin benchmark -- --start 2020-02-01 --end 2020-04-30
```

The strategy must be profitable (or at least not lose too much) in ALL conditions.

### Step 3: Multi-symbol test

```bash
cargo run --bin benchmark -- --symbols "AAPL,MSFT,GOOGL,AMZN,META"
```

Verify result consistency across different assets.

### Step 4: Stress test

Test on crash periods:
- **COVID crash**: February-March 2020
- **2022 Bear market**: January-October 2022
- **Flash crashes**: Verify resilience

## Pitfalls to avoid

### Overfitting

**Symptoms:**
- Sharpe Ratio > 3 on backtest
- Performance degrades in live/forward test
- Too many optimized parameters

**Solutions:**
- Use train/test split
- Test on out-of-sample data
- Prefer simple strategies

### Look-ahead bias

**Symptom:** Using future data in decisions

**Solution:** Verify indicators only use past data

### Survivorship bias

**Symptom:** Only testing on assets that still exist

**Solution:** Include delisted assets in backtests

## Key files

| File | Description |
|------|-------------|
| `src/bin/benchmark.rs` | Main benchmark CLI |
| `src/bin/benchmark_matrix.rs` | Parameter matrix tests |
| `src/application/optimization/parallel_benchmark.rs` | Parallel execution |
| `src/application/optimization/benchmark_metrics.rs` | Benchmark metrics |
| `src/domain/performance/metrics.rs` | Sharpe, Sortino, Drawdown calculation |
| `benchmark_results/` | Saved results |

## Checklist before production

- [ ] Positive backtests on 2+ years of data
- [ ] Sharpe Ratio > 1.0 on different periods
- [ ] Acceptable Max Drawdown (< 20% recommended)
- [ ] Tested on bull, bear AND sideways markets
- [ ] No sign of overfitting
- [ ] Paper trading validated for 1+ month
