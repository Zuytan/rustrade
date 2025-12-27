#!/bin/bash
# Debug script to run the bot with maximum logging

# Export enhanced logging
export RUST_LOG=debug

# Run the bot
echo "Starting Rustrade bot with debug logging..."
echo "==========================================="
echo ""

cargo run --release --bin rustrade 2>&1 | tee rustrade_debug.log
