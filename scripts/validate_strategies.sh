#!/bin/bash
# Backtest Validation Script for Rustrade
# Automatically validates trading strategies against historical data
# Fails if any strategy doesn't meet minimum performance thresholds

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "================================================"
echo "  Rustrade Strategy Validation Suite"
echo "================================================"
echo ""

# Configuration
SYMBOLS=("SPY" "QQQ" "AAPL" "MSFT" "TSLA")
START_DATE="2023-01-01"
END_DATE="2024-12-31"
MIN_SHARPE=1.0
MIN_WIN_RATE=0.40
MIN_PROFIT_FACTOR=1.5
MAX_DRAWDOWN=0.25
MIN_TRADES=30

# Results tracking
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

echo "Test Configuration:"
echo "  Symbols: ${SYMBOLS[@]}"
echo "  Period: ${START_DATE} to ${END_DATE}"
echo "  Min Sharpe Ratio: ${MIN_SHARPE}"
echo "  Min Win Rate: ${MIN_WIN_RATE} ($(echo "$MIN_WIN_RATE * 100" | bc)%)"
echo "  Min Profit Factor: ${MIN_PROFIT_FACTOR}"
echo "  Max Drawdown: ${MAX_DRAWDOWN} ($(echo "$MAX_DRAWDOWN * 100" | bc)%)"
echo "  Min Trades: ${MIN_TRADES}"
echo ""
echo "================================================"
echo ""

# Create results directory
RESULTS_DIR="validation_results_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

# Function to validate a single symbol
validate_symbol() {
    local symbol=$1
    local test_num=$2
    
    echo -e "${YELLOW}[$test_num/${#SYMBOLS[@]}] Testing $symbol...${NC}"
    
    # Run backtest
    echo "  Running backtest..."
    cargo run --bin benchmark --release -- \
        --symbol "$symbol" \
        --start "$START_DATE" \
        --end "$END_DATE" \
        > "$RESULTS_DIR/${symbol}_backtest.txt" 2>&1
    
    if [ $? -ne 0 ]; then
        echo -e "  ${RED}✗ Backtest failed to run${NC}"
        return 1
    fi
    
    # Extract metrics from output
    SHARPE=$(grep "Sharpe Ratio:" "$RESULTS_DIR/${symbol}_backtest.txt" | awk '{print $3}')
    WIN_RATE=$(grep "Win Rate:" "$RESULTS_DIR/${symbol}_backtest.txt" | awk '{print $3}' | tr -d '%')
    PROFIT_FACTOR=$(grep "Profit Factor:" "$RESULTS_DIR/${symbol}_backtest.txt" | awk '{print $3}')
    DRAWDOWN=$(grep "Max Drawdown:" "$RESULTS_DIR/${symbol}_backtest.txt" | awk '{print $3}' | tr -d '%')
    TOTAL_TRADES=$(grep "Total Trades:" "$RESULTS_DIR/${symbol}_backtest.txt" | awk '{print $3}')
    
    # Convert percentages to decimals for comparison
    WIN_RATE_DECIMAL=$(echo "scale=4; $WIN_RATE / 100" | bc)
    DRAWDOWN_DECIMAL=$(echo "scale=4; $DRAWDOWN / 100" | bc)
    
    echo "  Metrics:"
    echo "    Sharpe Ratio: $SHARPE"
    echo "    Win Rate: ${WIN_RATE}%"
    echo "    Profit Factor: $PROFIT_FACTOR"
    echo "    Max Drawdown: ${DRAWDOWN}%"
    echo "    Total Trades: $TOTAL_TRADES"
    echo ""
    
    # Validation checks
    local failed=0
    local failures=()
    
    # Check 1: Minimum trades
    if [ "$TOTAL_TRADES" -lt "$MIN_TRADES" ]; then
        failures+=("INSUFFICIENT DATA: $TOTAL_TRADES trades < $MIN_TRADES minimum")
        failed=1
    fi
    
    # Check 2: Sharpe ratio
    if (( $(echo "$SHARPE < $MIN_SHARPE" | bc -l) )); then
        failures+=("LOW SHARPE: $SHARPE < $MIN_SHARPE minimum")
        failed=1
    fi
    
    # Check 3: Win rate
    if (( $(echo "$WIN_RATE_DECIMAL < $MIN_WIN_RATE" | bc -l) )); then
        failures+=("LOW WIN RATE: ${WIN_RATE}% < $(echo "$MIN_WIN_RATE * 100" | bc)% minimum")
        failed=1
    fi
    
    # Check 4: Profit factor
    if (( $(echo "$PROFIT_FACTOR < $MIN_PROFIT_FACTOR" | bc -l) )); then
        failures+=("LOW PROFIT FACTOR: $PROFIT_FACTOR < $MIN_PROFIT_FACTOR minimum")
        failed=1
    fi
    
    # Check 5: Max drawdown
    if (( $(echo "$DRAWDOWN_DECIMAL > $MAX_DRAWDOWN" | bc -l) )); then
        failures+=("EXCESSIVE DRAWDOWN: ${DRAWDOWN}% > $(echo "$MAX_DRAWDOWN * 100" | bc)% maximum")
        failed=1
    fi
    
    if [ $failed -eq 1 ]; then
        echo -e "  ${RED}✗ VALIDATION FAILED${NC}"
        for failure in "${failures[@]}"; do
            echo -e "    ${RED}→ $failure${NC}"
        done
        echo ""
        return 1
    else
        echo -e "  ${GREEN}✓ VALIDATION PASSED${NC}"
        echo -e "    ${GREEN}All checks passed (5/5)${NC}"
        echo ""
        return 0
    fi
}

# Run validation for each symbol
for i in "${!SYMBOLS[@]}"; do
    symbol="${SYMBOLS[$i]}"
    test_num=$((i + 1))
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    
    if validate_symbol "$symbol" "$test_num"; then
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
done

# Generate summary report
echo "================================================"
echo "  Validation Summary"
echo "================================================"
echo ""
echo "Results:"
echo "  Total Tests: $TOTAL_TESTS"
echo -e "  ${GREEN}Passed: $PASSED_TESTS${NC}"
echo -e "  ${RED}Failed: $FAILED_TESTS${NC}"
echo ""
echo "Results saved to: $RESULTS_DIR/"
echo ""

# Generate detailed report
REPORT_FILE="$RESULTS_DIR/validation_report.txt"
cat > "$REPORT_FILE" << EOF
Rustrade Strategy Validation Report
Generated: $(date)

Configuration:
- Symbols: ${SYMBOLS[@]}
- Period: ${START_DATE} to ${END_DATE}
- Min Sharpe Ratio: ${MIN_SHARPE}
- Min Win Rate: ${MIN_WIN_RATE} ($(echo "$MIN_WIN_RATE * 100" | bc)%)
- Min Profit Factor: ${MIN_PROFIT_FACTOR}
- Max Drawdown: ${MAX_DRAWDOWN} ($(echo "$MAX_DRAWDOWN * 100" | bc)%)
- Min Trades: ${MIN_TRADES}

Results Summary:
- Total Tests: $TOTAL_TESTS
- Passed: $PASSED_TESTS
- Failed: $FAILED_TESTS
- Success Rate: $(echo "scale=1; $PASSED_TESTS * 100 / $TOTAL_TESTS" | bc)%

Detailed Results:
EOF

for symbol in "${SYMBOLS[@]}"; do
    echo "  $symbol:" >> "$REPORT_FILE"
    grep -A 10 "Testing $symbol" "$RESULTS_DIR/${symbol}_backtest.txt" >> "$REPORT_FILE" || echo "    No data" >> "$REPORT_FILE"
    echo "" >> "$REPORT_FILE"
done

echo "Detailed report: $REPORT_FILE"
echo ""

# Exit with failure if any tests failed
if [ $FAILED_TESTS -gt 0 ]; then
    echo -e "${RED}================================================${NC}"
    echo -e "${RED}  VALIDATION FAILED: $FAILED_TESTS/$TOTAL_TESTS tests failed${NC}"
    echo -e "${RED}================================================${NC}"
    echo ""
    echo "Strategies do NOT meet minimum performance requirements."
    echo "Review failed tests and improve strategies before deployment."
    exit 1
else
    echo -e "${GREEN}================================================${NC}"
    echo -e "${GREEN}  ✓ ALL VALIDATION TESTS PASSED${NC}"
    echo -e "${GREEN}================================================${NC}"
    echo ""
    echo "All strategies meet minimum performance requirements."
    echo "Strategies are approved for deployment."
    exit 0
fi
