#!/bin/bash
set -e

# Comprehensive Benchmark Validation Script
# Tests risk-based min_profit_ratio across multiple periods and symbols

RESULTS_FILE="benchmark_results/risk_filter_validation_$(date +%Y%m%d_%H%M%S).csv"
mkdir -p benchmark_results

echo "=========================================="
echo "Risk-Based Filter Validation Benchmark"
echo "=========================================="
echo "RISK_APPETITE_SCORE: $(grep RISK_APPETITE_SCORE .env | cut -d= -f2)"
echo "Testing Strategy vs Buy & Hold"
echo "Results will be saved to: $RESULTS_FILE"
echo ""

# CSV Header
echo "Period,Symbol,StartDate,EndDate,Strategy_Return%,Net_Profit,Trades" > "$RESULTS_FILE"

# Define test periods and symbols
PERIODS=(
    "Election_Rally:2024-11-06:2024-12-06"
    "Oct_2024:2024-10-01:2024-10-31"
    "Sep_2024:2024-09-01:2024-09-30"
    "Aug_2024:2024-08-01:2024-08-31"
)

SYMBOLS=("AAPL" "NVDA" "TSLA" "MSFT" "JPM" "GOOGL" "AMZN" "META")

total_tests=$((${#PERIODS[@]} * ${#SYMBOLS[@]}))
current=0

for period_entry in "${PERIODS[@]}"; do
    IFS=':' read -r period_name start_date end_date <<< "$period_entry"
    
    for symbol in "${SYMBOLS[@]}"; do
        current=$((current + 1))
        echo "[$current/$total_tests] Testing $symbol ($period_name: $start_date to $end_date)..."
        
        # Run benchmark and capture output
        output=$(cargo run --release --bin benchmark -- \
            --symbol "$symbol" \
            --start "$start_date" \
            --end "$end_date" \
            --strategy advanced 2>&1 | tail -1)
        
        # Parse output (format: "Return: X.XX% | Net: $XXX.XX | Trades: X")
        if echo "$output" | grep -q "Return:"; then
            strategy_return=$(echo "$output" | sed -n 's/.*Return: \([0-9.-]*\)%.*/\1/p')
            net_profit=$(echo "$output" | sed -n 's/.*Net: \$\([0-9.-]*\).*/\1/p')
            trades=$(echo "$output" | sed -n 's/.*Trades: \([0-9]*\).*/\1/p')
            
            echo "$period_name,$symbol,$start_date,$end_date,$strategy_return,$net_profit,$trades" >> "$RESULTS_FILE"
            echo "  ✓ Return: ${strategy_return}%, Net: \$$net_profit, Trades: $trades"
        else
            echo "$period_name,$symbol,$start_date,$end_date,ERROR,ERROR,ERROR" >> "$RESULTS_FILE"
            echo "  ✗ Failed - skipping"
        fi
    done
    echo ""
done

echo "=========================================="
echo "Benchmark Complete!"
echo "=========================================="
echo "Results saved to: $RESULTS_FILE"
echo ""
echo "Summary Statistics:"
awk -F',' 'NR>1 && $5!="ERROR" {sum+=$5; count++; trades+=$7; if($5>0) wins++} END {
    printf "Tests Completed: %d\n", count
    printf "Average Return: %.2f%%\n", sum/count
    printf "Win Rate: %.1f%%\n", (wins/count)*100
    printf "Total Trades: %d\n", trades
}' "$RESULTS_FILE"

echo ""
echo "Full results in: $RESULTS_FILE"
