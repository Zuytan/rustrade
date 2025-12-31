# Strategy Validation Scripts

This directory contains automated validation scripts for testing trading strategies.

## Available Scripts

### `validate_strategies.sh`

Automated backtest validation script that tests strategies against historical data.

**Purpose**: Ensure strategies meet minimum performance thresholds before deployment.

**Usage**:
```bash
./scripts/validate_strategies.sh
```

**What it does**:
1. Tests strategies on multiple symbols (SPY, QQQ, AAPL, MSFT, TSLA)
2. Runs backtests on 2023-2024 historical data
3. Validates against performance thresholds:
   - Sharpe Ratio ≥ 1.0
   - Win Rate ≥ 40%
   - Profit Factor ≥ 1.5
   - Max Drawdown ≤ 25%
   - Minimum 30 trades
4. Generates detailed validation report
5. Exits with error code if any validation fails

**Output**:
- Console: Color-coded pass/fail results
- Files: `validation_results_YYYYMMDD_HHMMSS/`
  - Individual backtest results per symbol
  - Aggregated validation report

**Exit Codes**:
- `0`: All validations passed
- `1`: One or more validations failed

**CI/CD Integration**:
```yaml
# .github/workflows/validate.yml
- name: Validate Strategies
  run: ./scripts/validate_strategies.sh
```

## Configuration

Edit the script to customize validation parameters:

```bash
# Which symbols to test
SYMBOLS=("SPY" "QQQ" "AAPL" "MSFT" "TSLA")

# Historical data period
START_DATE="2023-01-01"
END_DATE="2024-12-31"

# Performance thresholds
MIN_SHARPE=1.0
MIN_WIN_RATE=0.40
MIN_PROFIT_FACTOR=1.5
MAX_DRAWDOWN=0.25
MIN_TRADES=30
```

## Requirements

- Rust toolchain
- `bc` command (for calculations)
- Benchmark binary (`cargo build --bin benchmark`)

## Example Output

```
================================================
  Rustrade Strategy Validation Suite
================================================

Test Configuration:
  Symbols: SPY QQQ AAPL MSFT TSLA
  Period: 2023-01-01 to 2024-12-31
  Min Sharpe Ratio: 1.0
  Min Win Rate: 0.40 (40%)
  Min Profit Factor: 1.5
  Max Drawdown: 0.25 (25%)
  Min Trades: 30

================================================

[1/5] Testing SPY...
  Running backtest...
  Metrics:
    Sharpe Ratio: 1.2
    Win Rate: 45%
    Profit Factor: 1.8
    Max Drawdown: 18%
    Total Trades: 42
  ✓ VALIDATION PASSED
    All checks passed (5/5)

[2/5] Testing QQQ...
  Running backtest...
  Metrics:
    Sharpe Ratio: 0.8
    Win Rate: 52%
    Profit Factor: 2.1
    Max Drawdown: 15%
    Total Trades: 38
  ✗ VALIDATION FAILED
    → LOW SHARPE: 0.8 < 1.0 minimum

...

================================================
  Validation Summary
================================================

Results:
  Total Tests: 5
  Passed: 4
  Failed: 1

Results saved to: validation_results_20250101_120000/

================================================
  VALIDATION FAILED: 1/5 tests failed
================================================

Strategies do NOT meet minimum performance requirements.
Review failed tests and improve strategies before deployment.
```
