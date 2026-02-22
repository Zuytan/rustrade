#!/usr/bin/env bash

# Trading Code Review Script
# Automated checks for financial safety and code quality in Rustrade
# 
# Usage: ./scripts/review_trading_code.sh [path]
# Example: ./scripts/review_trading_code.sh src/application/strategies/

set -e

# Colors for output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Counters
BLOCKERS=0
WARNINGS=0
PASSED=0

# Default path to check
CHECK_PATH="${1:-src/application/strategies/}"

echo -e "${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║          RUSTRADE TRADING CODE REVIEW SCRIPT                 ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "Checking path: $CHECK_PATH"
echo ""

# Function to print section header
print_section() {
    echo ""
    echo -e "${BLUE}═══ $1 ═══${NC}"
    echo ""
}

# Function to print blocker
print_blocker() {
    echo -e "${RED}⛔ BLOCKER: $1${NC}"
    ((BLOCKERS++))
}

# Function to print warning
print_warning() {
    echo -e "${YELLOW}🟡 WARNING: $1${NC}"
    ((WARNINGS++))
}

# Function to print pass
print_pass() {
    echo -e "${GREEN}✅ PASSED: $1${NC}"
    ((PASSED++))
}

# ============================================================================
# CHECK 1: Float types in financial code (CRITICAL)
# ============================================================================
print_section "1. MONETARY PRECISION CHECK (CRITICAL)"

echo "Searching for f64/f32 usage in financial code..."
FLOAT_USAGE=$(grep -r -n -E "(: f64|: f32|-> f64|-> f32)" \
    src/application/strategies/ \
    src/domain/trading/ \
    src/domain/risk/ \
    2>/dev/null || true)

if [ ! -z "$FLOAT_USAGE" ]; then
    print_blocker "Float types (f64/f32) found in financial code"
    echo ""
    echo "Violations:"
    echo "$FLOAT_USAGE" | head -10
    echo ""
    echo "Rule: NEVER use f64/f32 for monetary calculations. Use rust_decimal::Decimal."
    echo ""
else
    print_pass "No f64/f32 usage in financial code"
fi

# ============================================================================
# CHECK 2: Unwrap usage (WARNING)
# ============================================================================
print_section "2. ERROR HANDLING CHECK"

echo "Searching for .unwrap() in production code..."
UNWRAP_USAGE=$(find src/application/strategies src/domain/trading src/domain/risk \
    -name "*.rs" ! -name "*test*.rs" ! -name "*tests.rs" \
    -exec grep -Hn "\.unwrap()" {} \; 2>/dev/null | head -20 || true)

if [ ! -z "$UNWRAP_USAGE" ]; then
    print_warning ".unwrap() found in production code"
    echo ""
    echo "Found $(echo "$UNWRAP_USAGE" | wc -l) instances (showing first 10):"
    echo "$UNWRAP_USAGE" | head -10
    echo ""
    echo "Recommendation: Use ? operator or .expect() with context message."
    echo ""
else
    print_pass "No .unwrap() in production code"
fi

# ============================================================================
# CHECK 3: Stop loss implementation
# ============================================================================
print_section "3. RISK MANAGEMENT - STOP LOSS CHECK"

if [ -d "src/application/strategies/" ]; then
    echo "Checking for stop loss implementation..."
    
    # Count strategy implementations
    STRATEGY_IMPLS=$(grep -r "impl TradingStrategy for" src/application/strategies/ 2>/dev/null | wc -l)
    STOP_LOSS_USAGE=$(grep -r "with_stop_loss" src/application/strategies/ 2>/dev/null | wc -l)
    
    echo "Strategy implementations found: $STRATEGY_IMPLS"
    echo "Stop loss usages found: $STOP_LOSS_USAGE"
    echo ""
    
    if [ $STOP_LOSS_USAGE -eq 0 ] && [ $STRATEGY_IMPLS -gt 0 ]; then
        print_warning "No stop loss implementation detected"
        echo "Requirement: All strategies must set stop losses using .with_stop_loss()"
    else
        print_pass "Stop loss implementation detected"
    fi
else
    echo "Strategies directory not found, skipping..."
fi

# ============================================================================
# CHECK 4: Hardcoded quantities
# ============================================================================
print_section "4. POSITION SIZING CHECK"

echo "Checking for hardcoded quantities..."
HARDCODED_QTY=$(grep -r -n -E "quantity.*=.*Decimal::from\([0-9]+\)" \
    src/application/strategies/ 2>/dev/null || true)

if [ ! -z "$HARDCODED_QTY" ]; then
    print_warning "Potential hardcoded quantities detected"
    echo ""
    echo "$HARDCODED_QTY"
    echo ""
    echo "Recommendation: Position sizing should be dynamic and risk-based."
else
    print_pass "No hardcoded quantities found"
fi

# ============================================================================
# CHECK 5: Direct order execution (CRITICAL)
# ============================================================================
print_section "5. ARCHITECTURAL COMPLIANCE CHECK"

echo "Checking for direct order execution in strategies..."
DIRECT_EXEC=$(grep -r -n -E "(place_order|execute_order|submit_order)" \
    src/application/strategies/ 2>/dev/null || true)

if [ ! -z "$DIRECT_EXEC" ]; then
    print_blocker "Direct order execution found in strategies"
    echo ""
    echo "$DIRECT_EXEC"
    echo ""
    echo "Rule: Strategies must NOT execute orders. They should only return Signal."
else
    print_pass "No direct order execution in strategies"
fi

# ============================================================================
# CHECK 6: Test coverage
# ============================================================================
print_section "6. TEST COVERAGE CHECK"

if [ -d "src/application/strategies/" ]; then
    STRATEGY_FILES=$(find src/application/strategies -name "*.rs" -type f | wc -l)
    TEST_MODULES=$(grep -r "#\[cfg(test)\]" src/application/strategies/ 2>/dev/null | wc -l)
    
    echo "Strategy files: $STRATEGY_FILES"
    echo "Test modules: $TEST_MODULES"
    echo ""
    
    if [ $TEST_MODULES -eq 0 ] && [ $STRATEGY_FILES -gt 0 ]; then
        print_warning "No test modules found"
        echo "Recommendation: Add unit tests for all strategies."
    else
        print_pass "Test modules present"
    fi
fi

# ============================================================================
# CHECK 7: Use of ta crate indicators
# ============================================================================
print_section "7. INDICATOR IMPLEMENTATION CHECK"

echo "Checking for custom indicator implementations..."

# Check if strategies use ta crate
TA_USAGE=$(grep -r "use ta::" src/application/strategies/ 2>/dev/null | wc -l)

echo "ta crate imports found: $TA_USAGE"

if [ $TA_USAGE -eq 0 ]; then
    print_warning "No ta crate usage detected"
    echo "Recommendation: Use ta crate for standard indicators (RSI, MACD, etc.)"
else
    print_pass "ta crate is being used"
fi

# ============================================================================
# CHECK 8: Run Clippy
# ============================================================================
print_section "8. RUNNING CLIPPY CHECKS"

echo "Running cargo clippy on trading code..."
if cargo clippy --all-targets -- -D warnings 2>&1 | grep -E "(error|warning)" | head -20; then
    print_warning "Clippy found issues"
else
    print_pass "Clippy checks passed"
fi

# ============================================================================
# SUMMARY
# ============================================================================
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}                      REVIEW SUMMARY                            ${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""

echo -e "${RED}⛔ Blockers: $BLOCKERS${NC}"
echo -e "${YELLOW}🟡 Warnings: $WARNINGS${NC}"
echo -e "${GREEN}✅ Passed: $PASSED${NC}"
echo ""

if [ $BLOCKERS -gt 0 ]; then
    echo -e "${RED}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${RED}║  ⛔ CANNOT MERGE: Critical blockers must be resolved        ║${NC}"
    echo -e "${RED}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo "See REVIEW_GUIDELINES.md for detailed requirements."
    exit 1
elif [ $WARNINGS -gt 0 ]; then
    echo -e "${YELLOW}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${YELLOW}║  🟡 WARNINGS: Review carefully before merging               ║${NC}"
    echo -e "${YELLOW}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo "Manual review recommended. See REVIEW_GUIDELINES.md"
    exit 0
else
    echo -e "${GREEN}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║  ✅ ALL CHECKS PASSED - Ready for manual review             ║${NC}"
    echo -e "${GREEN}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo "Note: Automated checks passed, but manual quantitative review is still required."
    exit 0
fi
