#!/bin/bash

# Dynamic Benchmark Suite
# Runs the benchmark tool across different market regimes to validate strategy adaptability.

echo "================================================================="
echo "📊 STARTING DYNAMIC REGIME BENCHMARKS"
echo "================================================================="

# 1. Bull Market Start (Jan 2024)
echo ""
echo ">>> PERIOD 1: Bull Market Start (Post-Holidays)"
echo ">>> Date: 2024-01-03 | Duration: 30 Days"
cargo run --release --bin benchmark -- \
  --historical-scan 2024-01-03 \
  --days 30

# 2. Correction / Volatility (April 2024)
echo ""
echo ">>> PERIOD 2: Correction / Volatility"
echo ">>> Date: 2024-04-15 | Duration: 30 Days"
cargo run --release --bin benchmark -- \
  --historical-scan 2024-04-15 \
  --days 30

# 3. Flash Crash (August 2024)
echo ""
echo ">>> PERIOD 3: Flash Crash (Yen Carry Unwind)"
echo ">>> Date: 2024-08-05 | Duration: 30 Days"
cargo run --release --bin benchmark -- \
  --historical-scan 2024-08-05 \
  --days 30

# 4. Election Rally (Nov 2024)
echo ""
echo ">>> PERIOD 4: Election Rally"
echo ">>> Date: 2024-11-06 | Duration: 30 Days"
cargo run --release --bin benchmark -- \
  --historical-scan 2024-11-06 \
  --days 30

echo ""
echo "================================================================="
echo "✅ BENCHMARK SUITE COMPLETED"
echo "================================================================="
