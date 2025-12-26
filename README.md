# RustTrade Agentic Bot 🦀

A high-performance, multi-agent algorithmic trading system built in Rust. Capable of real-time market surveillance, trend analysis, and autonomous execution.

## 🚀 Key Features
- **Multi-Agent Architecture**: Dedicated agents for Sentinel (Data), Analyst (Strategy), Risk Manager, and Execution.
- **Real-Time Analysis**: Dual SMA & Triple Filter (Trend/RSI/MACD) strategies processed on live WebSocket data.
- **Safety First**: "Strict Decimal" policy, PDT protection, and **Real-time Circuit Breakers** (active valuation loop).
- **Backtesting & Benchmark**: Integrated historical simulation engine for strategy verification.

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
cargo run

# 3. Run (Alpaca Paper Mode)
MODE=alpaca cargo run
```

### Running Benchmarks
```bash
# Config for benchmark
cp .env.benchmark .env.benchmark.local

# Run Strategy Test
cargo run --bin benchmark -- --symbol NVDA --start 2024-01-01 --strategy advanced
```
