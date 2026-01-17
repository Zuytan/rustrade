#!/bin/bash
# Quick benchmark for a single symbol
# Usage: ./quick_benchmark.sh SYMBOL [DAYS]
#
# Example: ./quick_benchmark.sh AAPL 365

set -e

SYMBOL=${1:-AAPL}
DAYS=${2:-365}

echo "=================================================="
echo "🚀 Quick Benchmark: $SYMBOL ($DAYS days)"
echo "=================================================="

cd "$(dirname "$0")/../../../.."

cargo run --release --bin benchmark -- --symbol "$SYMBOL" --days "$DAYS"
