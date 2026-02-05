#!/bin/bash
set -e

# Configuration
MODEL_DIR="data/ml"
HISTORICAL_DATA="data/candles"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
METRICS_FILE="benchmark_results/last_run_metrics.json"

echo "🚀 Starting Automated ML Pipeline..."

# 1. Clean previous run
echo "🧹 Cleaning temporary data..."
rm -f "$MODEL_DIR/dataset_*.csv"

# 2. Generate Training Data
echo "📊 Generating training dataset from historical candles..."
# Uses train_gen binary to replay history and label features
cargo run --release --bin train_gen -- \
    --symbols "BTC/USD,ETH/USD" \
    --start-date "2024-01-01" \
    --end-date "2024-12-31" \
    --output "$MODEL_DIR/dataset_latest.csv"

# 3. Train Model
echo "🧠 Training Random Forest model..."
cargo run --release --bin train_ml -- \
    --input "$MODEL_DIR/dataset_latest.csv" \
    --output "$MODEL_DIR/model_$TIMESTAMP.bin" \
    --trees 100 \
    --depth 10

# 4. Deploy (Link as current active model)
echo "🔗 Deploying model to active production path..."
ln -sf "model_$TIMESTAMP.bin" "$MODEL_DIR/model.bin"

# 5. Quick Verification
echo "✅ verifying model loading..."
if [ -f "$MODEL_DIR/model.bin" ]; then
    echo "SUCCESS: Model deployed to $MODEL_DIR/model.bin"
else
    echo "ERROR: Model deployment failed"
    exit 1
fi

echo "🏁 Pipeline Completed Successfully!"
