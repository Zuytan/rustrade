#!/bin/bash
set -e

# ============================================================================
# Multi-Stock Benchmark Evaluation Script
# ============================================================================
# Purpose: Comprehensive performance testing across diverse stock symbols
# Output: CSV results for easy analysis
# ============================================================================

# Configuration
RESULTS_DIR="benchmark_results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_FILE="${RESULTS_DIR}/stocks_${TIMESTAMP}.csv"
LOG_FILE="${RESULTS_DIR}/stocks_${TIMESTAMP}.log"

# Ensure .env.benchmark is used
if [ ! -f .env.benchmark ]; then
    echo "Creating .env.benchmark from alpaca.env..."
    cp alpaca.env .env.benchmark
fi

# Create results directory
mkdir -p "${RESULTS_DIR}"

echo "=============================================================================" | tee "${LOG_FILE}"
echo "🚀 MULTI-STOCK BENCHMARK EVALUATION - $(date)" | tee -a "${LOG_FILE}"
echo "=============================================================================" | tee -a "${LOG_FILE}"
echo "" | tee -a "${LOG_FILE}"

# Stock Selection by Sector (21 stocks total)
ALL_STOCKS=(
    # Tech (5)
    "AAPL" "MSFT" "GOOGL" "NVDA" "META"
    # Mega Cap (2)
    "AMZN" "TSLA"
    # Finance (4)
    "JPM" "BAC" "V" "MA"
    # Energy (2)
    "XOM" "CVX"
    # Healthcare (3)
    "JNJ" "ABBV" "LLY"
    # Consumer (3)
    "WMT" "COST" "KO"
    # Industrial (2)
    "CAT" "GE"
)

# Test Periods (using single period for faster execution)
START_DATE="2024-11-06"
END_DATE="2024-12-06"
PERIOD_NAME="Election Rally"

# Initialize CSV with headers
echo "Symbol,Period,StartDate,EndDate,ReturnPct,BuyHoldPct,NetProfit,Trades,Status" > "${RESULTS_FILE}"

TOTAL_BENCHMARKS=${#ALL_STOCKS[@]}
CURRENT_BENCHMARK=0

echo "📊 Testing ${#ALL_STOCKS[@]} stocks for ${PERIOD_NAME} period" | tee -a "${LOG_FILE}"
echo "📅 Period: ${START_DATE} → ${END_DATE}" | tee -a "${LOG_FILE}"
echo "" | tee -a "${LOG_FILE}"

# Progress bar function
show_progress() {
    local current=$1
    local total=$2
    local percent=$((current * 100 / total))
    local filled=$((percent / 2))
    local empty=$((50 - filled))
    
    printf "\r["
    printf "%${filled}s" | tr ' ' '='
    printf "%${empty}s" | tr ' ' '-'
    printf "] %3d%% (%d/%d)" "$percent" "$current" "$total"
}

# Execute benchmarks
for symbol in "${ALL_STOCKS[@]}"; do
    CURRENT_BENCHMARK=$((CURRENT_BENCHMARK + 1))
    
    echo "" | tee -a "${LOG_FILE}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" | tee -a "${LOG_FILE}"
    echo "[$CURRENT_BENCHMARK/$TOTAL_BENCHMARKS] Testing ${symbol}" | tee -a "${LOG_FILE}"
    
    # Run benchmark
    if cargo run --release --bin benchmark -- \
        --symbol "${symbol}" \
        --start "${START_DATE}" \
        --end "${END_DATE}" \
        --strategy advanced > /tmp/benchmark_output.txt 2>&1; then
        
        # Parse output for metrics
        RETURN_PCT=$(grep "Return:" /tmp/benchmark_output.txt | grep -oE "[0-9.-]+" | head -1 || echo "0.0")
        NET_PROFIT=$(grep "Net:" /tmp/benchmark_output.txt | grep -oE "[0-9.-]+" | head -1 || echo "0.0")
        TRADES=$(grep "Trades:" /tmp/benchmark_output.txt | grep -oE "[0-9]+" | head -1 || echo "0")
        
        # Note: Buy & Hold % not easily extractable from current output format
        BH_PCT="N/A"
        
        # Append to CSV
        echo "${symbol},${PERIOD_NAME},${START_DATE},${END_DATE},${RETURN_PCT},${BH_PCT},${NET_PROFIT},${TRADES},success" >> "${RESULTS_FILE}"
        
        echo "   ✅ Return: ${RETURN_PCT}% | Net: \$${NET_PROFIT} | Trades: ${TRADES}" | tee -a "${LOG_FILE}"
    else
        echo "${symbol},${PERIOD_NAME},${START_DATE},${END_DATE},0.0,N/A,0.0,0,failed" >> "${RESULTS_FILE}"
        echo "   ❌ Benchmark failed (check log for details)" | tee -a "${LOG_FILE}"
        cat /tmp/benchmark_output.txt >> "${LOG_FILE}"
    fi
    
    # Show progress
    show_progress "$CURRENT_BENCHMARK" "$TOTAL_BENCHMARKS"
done

echo "" | tee -a "${LOG_FILE}"
echo "" | tee -a "${LOG_FILE}"
echo "=============================================================================" | tee -a "${LOG_FILE}"
echo "✅ BENCHMARK EVALUATION COMPLETE" | tee -a "${LOG_FILE}"
echo "=============================================================================" | tee -a "${LOG_FILE}"
echo "" | tee -a "${LOG_FILE}"
echo "Results saved to: ${RESULTS_FILE}" | tee -a "${LOG_FILE}"
echo "Log saved to: ${LOG_FILE}" | tee -a "${LOG_FILE}"
echo "" | tee -a "${LOG_FILE}"

# Quick summary
SUCCESSFUL=$(grep -c "success" "${RESULTS_FILE}" || echo "0")
FAILED=$(grep -c "failed" "${RESULTS_FILE}" || echo "0")

echo "📊 QUICK SUMMARY:" | tee -a "${LOG_FILE}"
echo "   Total Benchmarks: ${TOTAL_BENCHMARKS}" | tee -a "${LOG_FILE}"
echo "   Successful: ${SUCCESSFUL}" | tee -a "${LOG_FILE}"
echo "   Failed: ${FAILED}" | tee -a "${LOG_FILE}"
echo "" | tee -a "${LOG_FILE}"
