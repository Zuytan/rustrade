#!/bin/bash

# Dynamic Benchmark Suite
# Runs the benchmark tool across different market regimes to validate strategy adaptability.

echo "================================================================="
echo "📊 STARTING DYNAMIC REGIME BENCHMARKS"
echo "================================================================="


# Define Risk Profiles to test
# Score 2: Conservative (Low risk, tight stops)
# Score 5: Balanced (Medium risk)
# Score 8: Aggressive (High risk, loose stops)
RISK_PROFILES=(2 5 8)
PROFILE_NAMES=("Conservative" "Balanced" "Aggressive")

for i in "${!RISK_PROFILES[@]}"; do
  SCORE="${RISK_PROFILES[$i]}"
  NAME="${PROFILE_NAMES[$i]}"

  echo "#################################################################"
  echo "⚖️  RUNNING BENCHMARKS WITH RISK PROFILE: $NAME (Score: $SCORE)"
  echo "#################################################################"

  export RISK_APPETITE_SCORE=$SCORE

  # 1. Bull Market Start (Jan 2024)
  echo ""
  echo ">>> PERIOD 1: Bull Market Start (Post-Holidays)"
  echo ">>> Date: 2024-01-03 | Duration: 30 Days"
  cargo run --release --bin benchmark -- \
    --historical-scan 2024-01-03 \
    --strategy regimeadaptive \
    --days 30

  # 2. Correction / Volatility (April 2024)
  echo ""
  echo ">>> PERIOD 2: Correction / Volatility"
  echo ">>> Date: 2024-04-15 | Duration: 30 Days"
  cargo run --release --bin benchmark -- \
    --historical-scan 2024-04-15 \
    --strategy regimeadaptive \
    --days 30

  # 3. Flash Crash (August 2024)
  echo ""
  echo ">>> PERIOD 3: Flash Crash (Yen Carry Unwind)"
  echo ">>> Date: 2024-08-05 | Duration: 30 Days"
  cargo run --release --bin benchmark -- \
    --historical-scan 2024-08-05 \
    --strategy regimeadaptive \
    --days 30

  # 4. Election Rally (Nov 2024)
  echo ""
  echo ">>> PERIOD 4: Election Rally"
  echo ">>> Date: 2024-11-06 | Duration: 30 Days"
  cargo run --release --bin benchmark -- \
    --historical-scan 2024-11-06 \
    --strategy regimeadaptive \
    --days 30
    
  echo ""
  echo "✅ COMPLETED PROFILE: $NAME"
  echo ""
done

echo ""
echo "================================================================="
echo "✅ BENCHMARK SUITE COMPLETED"
echo "================================================================="
