# RustTrade Agentic Bot 🦀

A high-performance, multi-agent algorithmic trading system built in Rust. Capable of real-time market surveillance, trend analysis, and autonomous execution.

## 🚀 Key Features
- **Multi-Agent Architecture**: 6 specialized agents (Sentinel, Scanner, Analyst, Risk Manager, Order Throttler, Executor)
- **5 Trading Strategies**: Standard Dual SMA, Advanced Triple Filter, Dynamic Regime Adaptive, Trend Riding, Mean Reversion
- **Real-Time Market Analysis**: WebSocket streaming with intelligent candle aggregation
- **Advanced Risk Management**: Circuit breakers, PDT protection, trailing stops (ATR-based)
- **Backtesting & Optimization**: 
  - Historical backtesting with S&P500 benchmark comparison
  - Alpha/Beta calculation vs market
  - Grid search parameter optimization
  - Comprehensive performance metrics (Sharpe, Sortino, Calmar)
- **Safety Features**: 
  - Strict `Decimal` arithmetic (no floating-point errors)
  - Multi-level safeguards (Position sizing, Max drawdown, Daily loss limits)
  - Order throttling and cooldown periods

## 🛠️ Technical Stack

### Core
- **Language**: Rust 2021 Edition
- **Runtime**: `tokio` (Asynchronous I/O, Channels, Actors)
- **Architecture**: Hexagonal (Ports & Adapters) + Actor Model

### Data & Networking
- **Market Data**: Alpaca API v2 (WebSocket & REST)
- **WebSockets**: `tokio-tungstenite`
- **HTTP Client**: `reqwest`
- **Serialization**: `serde`, `serde_json`

### Intelligence & Math
- **Technical Indicators**: `ta` crate (SMA, RSI, MACD, etc.)
- **Financial Math**: `rust_decimal` (Fixed-point arithmetic for zero precision loss)
- **Time**: `chrono` (UTC handling)

### Observability
- **Logging**: `tracing` (Structured logging)
- **Error Handling**: `anyhow`

## 📚 Documentation
- [Global App Description](GLOBAL_APP_DESCRIPTION.md): Full architecture details.
- [Strategy Guide](docs/guide_strategie_simplifie.md): Simplified explanation of trading logic.
- [Walkthrough](walkthrough.md): Usage guide for Benchmark and Backtesting tools.

## ⚡ Quick Start

### Prerequisites
- Rust (Cargo)
- Alpaca API Keys (Paper Trading)

### Running the Bot
```bash
# 1. Configure Credentials
cp .env.example .env

# 2. Run (Mock Mode)
# Edit .env with your Alpaca API keys

# 2. Run with a strategy
cargo run --bin rustrade -- --strategy advanced

# Available strategies: standard, advanced, dynamic, trendriding, meanreversion
```

### Backtest a Strategy

```bash
# Single backtest with alpha/beta vs S&P500
cargo run --bin benchmark -- \
  --symbol NVDA \
  --start 2020-01-01 \
  --end 2024-12-31 \
  --strategy trendriding

# Batch mode (30-day windows)
cargo run --bin benchmark -- \
  --symbol TSLA \
  --start 2023-01-01 \
  --end 2023-12-31 \
  --strategy advanced \
  --batch-days 30
```

### Optimize Strategy Parameters

```bash
# Create parameter grid (grid.toml already exists)
# Run optimization
cargo run --bin optimize -- \
  --symbol NVDA \
  --start 2020-01-01 \
  --end 2023-12-31 \
  --grid-config grid.toml \
  --output nvda_best_params.json \
  --top-n 10

# View results
cat nvda_best_params.json | jq '.[0]'  # Best configuration
```
