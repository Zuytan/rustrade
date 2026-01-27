#!/bin/bash
set -e

# Configuration
SYMBOL="BTC/USD"
DAYS=5
TRAIN_DATA="data/ml/training_data.csv"
MODEL_PATH="data/ml/model.bin"
BEST_MODEL_PATH="data/ml/best_model.bin"

# Hyperparameter Grid
TREES=(50 100 200)
DEPTHS=(8 12 16)
MIN_SPLITS=(5 10)

best_profit=-999999
best_config=""

echo "========================================================"
echo "🚀 Starting ML Hyperparameter Optimization"
echo "Symbol: $SYMBOL | Days: $DAYS"
echo "Grid: Trees=${TREES[*]}, Depths=${DEPTHS[*]}, Splits=${MIN_SPLITS[*]}"
echo "========================================================"

# Compile binaries once
echo "Compiling binaries..."
cargo build --release --bin train_ml --bin benchmark

for t in "${TREES[@]}"; do
    for d in "${DEPTHS[@]}"; do
        for s in "${MIN_SPLITS[@]}"; do
            echo "--------------------------------------------------------"
            echo "Testing Config: Trees=$t, Depth=$d, Split=$s"
            
            # 1. Train Model
            target/release/train_ml --input "$TRAIN_DATA" --output "$MODEL_PATH" --n-trees "$t" --max-depth "$d" --min-split "$s" > /dev/null
            
            # 2. Benchmark
            # Capture output to parse results
            output=$(target/release/benchmark run --strategy ml --days "$DAYS" --symbols "$SYMBOL" --asset-class crypto 2>&1)
            
            # 3. Parse Result (Extract Net PnL)
            # Look for the line containing the strategy name 'ml' in the summary table
            line=$(echo "$output" | grep "| ml" | head -n 1)
            
            if [[ -z "$line" ]]; then
                echo "❌ Benchmark failed or produced no output."
                # Print output for debugging if needed, but keep it brief or log to file
                echo "$output" | tail -n 5
                continue
            fi
            
            # Extract Net PnL (column 6 approx, usually formatted like "$   123.45")
            # We remove '$' and ',' to parse as float
            net_pnl_str=$(echo "$line" | awk -F'|' '{print $6}' | sed 's/[$,]//g' | xargs)
            
            # Validate if it's a number
            if ! [[ "$net_pnl_str" =~ ^-?[0-9]+(\.[0-9]+)?$ ]]; then
                echo "⚠️ Could not parse PnL from: $line"
                net_pnl=0
            else
                net_pnl=$net_pnl_str
            fi
            
            echo "📊 Result: PnL = \$$net_pnl"
            
            # Compare with best using bc for float comparison
            is_better=$(echo "$net_pnl > $best_profit" | bc -l)
            
            if [[ "$is_better" -eq 1 ]]; then
                echo "🌟 NEW BEST! Previous: \$$best_profit"
                best_profit=$net_pnl
                best_config="Trees=$t, Depth=$d, Split=$s"
                cp "$MODEL_PATH" "$BEST_MODEL_PATH"
            fi
        done
    done
done

echo "========================================================"
echo "🏆 Optimization Complete"
echo "Best Configuration: $best_config"
echo "Best Net PnL: \$$best_profit"
echo "Saved best model to: $BEST_MODEL_PATH"
echo "========================================================"

# Restore best model
if [[ -f "$BEST_MODEL_PATH" ]]; then
    cp "$BEST_MODEL_PATH" "$MODEL_PATH"
    echo "Restored best model to $MODEL_PATH for production use."
fi
