#!/bin/bash
set -e

# Configuration
RESULTS_FILE="benchmark_results.log"
DAYS=30
APP_BIN="cargo run --bin benchmark --"

# Dates to test (Start of periods)
# 1. Bear Market Phase 1 (Jan 2022)
BEAR_START="2022-01-03"
# 2. Bull Market Recovery (Nov 2023)
BULL_START="2023-11-01"
# 3. Recent Volatility (April 2024)
MIXED_START="2024-04-01"

echo "=========================================================" > $RESULTS_FILE
echo "🚀 AUTO BENCHMARK RUN - $(date)" >> $RESULTS_FILE
echo "=========================================================" >> $RESULTS_FILE

run_benchmark() {
    local scan_date=$1
    local name=$2
    
    echo "" | tee -a $RESULTS_FILE
    echo ">>> RUNNING BENCHMARK: $name (Scan Date: $scan_date) <<<" | tee -a $RESULTS_FILE
    
    # Run in historical scan mode
    # 1. Scan for movers on $scan_date
    # 2. Replay strategy for next $DAYS
    $APP_BIN --historical-scan $scan_date --days $DAYS >> $RESULTS_FILE 2>&1
    
    echo ">>> COMPLETED: $name" | tee -a $RESULTS_FILE
}

# Execute
echo "Starting Benchmarks..."

run_benchmark $BEAR_START "BEAR MARKET (2022)"
run_benchmark $BULL_START "BULL RALLY (2023)"
run_benchmark $MIXED_START "MIXED MARKET (2024)"

echo ""
echo "✅ All benchmarks complete! Check $RESULTS_FILE for details."
