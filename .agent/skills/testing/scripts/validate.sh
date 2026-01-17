#!/bin/bash
# Validation script for Rustrade
# Usage: ./validate.sh [--fix]
#
# This script runs the full validation pipeline:
# 1. Format check (or fix with --fix)
# 2. Clippy lint
# 3. Tests

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=================================================="
echo "🔍 Rustrade Validation Pipeline"
echo "=================================================="

# Step 1: Format
if [ "$1" == "--fix" ]; then
    echo -e "\n${YELLOW}📝 Step 1: Formatting code...${NC}"
    cargo fmt --all
    echo -e "${GREEN}✅ Code formatted${NC}"
else
    echo -e "\n${YELLOW}📝 Step 1: Checking format...${NC}"
    if cargo fmt --all -- --check; then
        echo -e "${GREEN}✅ Format OK${NC}"
    else
        echo -e "${RED}❌ Format check failed. Run with --fix to auto-format${NC}"
        exit 1
    fi
fi

# Step 2: Clippy
echo -e "\n${YELLOW}🔧 Step 2: Running Clippy...${NC}"
if cargo clippy --all-targets -- -D warnings; then
    echo -e "${GREEN}✅ Clippy OK${NC}"
else
    echo -e "${RED}❌ Clippy found issues${NC}"
    exit 1
fi

# Step 3: Tests
echo -e "\n${YELLOW}🧪 Step 3: Running tests...${NC}"
if cargo test; then
    echo -e "${GREEN}✅ Tests OK${NC}"
else
    echo -e "${RED}❌ Tests failed${NC}"
    exit 1
fi

echo ""
echo "=================================================="
echo -e "${GREEN}✅ All validations passed!${NC}"
echo "=================================================="
