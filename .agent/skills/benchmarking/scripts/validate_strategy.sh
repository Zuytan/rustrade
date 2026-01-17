#!/bin/bash
# Full validation benchmark for a strategy
# Usage: ./validate_strategy.sh STRATEGY_NAME
#
# This runs benchmarks on multiple periods to validate a strategy

set -e

STRATEGY=${1:-TrendRiding}

echo "=================================================="
echo "📊 Strategy Validation: $STRATEGY"
echo "=================================================="

cd "$(dirname "$0")/../../../.."

echo ""
echo "🔹 Period 1: Bull Market (2021)"
cargo run --release --bin benchmark -- --strategy "$STRATEGY" --start 2021-01-01 --end 2021-12-31 2>/dev/null || echo "Period 1 failed"

echo ""
echo "🔹 Period 2: Bear Market (2022)"
cargo run --release --bin benchmark -- --strategy "$STRATEGY" --start 2022-01-01 --end 2022-12-31 2>/dev/null || echo "Period 2 failed"

echo ""
echo "🔹 Period 3: Recovery (2023)"
cargo run --release --bin benchmark -- --strategy "$STRATEGY" --start 2023-01-01 --end 2023-12-31 2>/dev/null || echo "Period 3 failed"

echo ""
echo "🔹 Period 4: COVID Crash (Feb-Apr 2020)"
cargo run --release --bin benchmark -- --strategy "$STRATEGY" --start 2020-02-01 --end 2020-04-30 2>/dev/null || echo "Period 4 failed"

echo ""
echo "=================================================="
echo "✅ Strategy validation complete"
echo "=================================================="
