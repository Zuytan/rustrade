# Rustrade Performance Benchmark Report

## Status: No Demonstrated Edge Yet

**No strategy has shown a reproducible, out-of-sample edge so far.** Rustrade must not be
used with real money until the validation criteria below are met.

> Earlier versions of this document showed a "Preliminary Results (Sample)" table
> (e.g. +120% on NVDA). Those figures were illustrative placeholders, not the output of an
> actual run, and have been removed. Only results produced by the benchmark binary
> belong in this report.

## Latest Recorded Run (2026-06-21)

- **Command**: `benchmark run --strategy regimeadaptive --asset-class crypto --risk 5`
- **Window**: 2024-12-20 → 2025-03-20 (3 months, mostly bearish crypto market)
- **Source**: `benchmark_results/benchmark_report_20260621_123817.json` (not versioned)

| Symbol | Return | Buy & Hold | Trades | Win Rate | Max DD |
|--------|-------:|-----------:|-------:|---------:|-------:|
| BTC/USD | -0.67% | -10.91% | 35 | 25.0% | 0.38% |
| ETH/USD | -3.93% | -39.88% | 113 | 31.3% | 0.78% |
| SOL/USD | -9.12% | -30.27% | 165 | 24.1% | 2.28% |
| XRP/USD | -14.64% | +14.10% | 280 | 29.8% | 2.93% |
| AVAX/USD | -4.48% | -49.39% | 240 | 39.0% | 1.13% |
| LINK/USD | +5.28% | -34.40% | 264 | 28.9% | 2.59% |
| DOT/USD | -9.72% | -34.88% | 172 | 18.2% | 2.33% |

**Summary**: 1 profitable scenario out of 7, average return -5.33%, average win rate 28%.

### Interpretation
- The strategy "beats" buy & hold only because it stayed mostly out of a falling market.
  On the one rising asset (XRP, +14.1%), it lost 14.6%.
- Trade counts (up to 280 in 3 months) combined with ~28% win rates point to
  **over-trading eroded by fees**, not to a predictive signal.
- A single 3-month bearish window is not a sufficient sample to conclude anything,
  positive or negative, about other regimes.

## Validation Criteria Before Any Live Capital

A strategy is considered validated only when all of the following hold:
1. **Walk-forward out-of-sample** results over at least 2 years, covering bull, bear and
   ranging regimes, with realistic fees and slippage.
2. Out-of-sample **Sharpe > 1** and **max drawdown < 15%**, with at least 100 trades.
3. Beats both **buy & hold** and a **naive baseline** (e.g. SMA crossover) after costs.
4. At least **2 months of paper trading** with live results within the backtest's
   expected range.

## How to Run Benchmarks

```bash
# Single strategy backtest (see --help for all options)
cargo run --release --bin benchmark -- run --help

# Parameter grid search
cargo run --release --bin benchmark -- matrix --help
```

Requires `ALPACA_API_KEY` and `ALPACA_SECRET_KEY` for historical data. JSON reports are
written to `benchmark_results/`. Update this document with actual results only.
