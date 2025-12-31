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

## ⚙️ Configuration

The application is configured primarily via environment variables. You can set these in your shell or use a `.env` file in the project root.

### Core & Connectivity
| Variable | Default | Description |
|----------|---------|-------------|
| `MODE` | `mock` | Trading mode: `mock`, `alpaca`, `oanda`. |
| `ASSET_CLASS` | `stock` | Asset class: `stock` or `crypto`. |
| `SYMBOLS` | `AAPL` | Comma-separated list of symbols to trade (e.g., `AAPL,TSLA`). |
| `ALPACA_API_KEY` | - | Your Alpaca API Key. |
| `ALPACA_SECRET_KEY` | - | Your Alpaca Secret Key. |
| `ALPACA_BASE_URL` | Paper URL | Alpaca API URL (Paper or Live). |
| `ALPACA_DATA_URL` | Data URL | Alpaca Data API URL. |
| `ALPACA_WS_URL` | Stream URL | Alpaca WebSocket URL. |
| `OANDA_API_KEY` | - | OANDA API Key (if mode is `oanda`). |
| `OANDA_ACCOUNT_ID` | - | OANDA Account ID. |

### Architecture & System
| Variable | Default | Description |
|----------|---------|-------------|
| `PORTFOLIO_STALENESS_MS` | `5000` | Max age of portfolio data before refresh (ms). |
| `MAX_ORDERS_PER_MINUTE` | `10` | Rate limiting for API calls. |
| `ORDER_COOLDOWN_SECONDS` | `300` | Minimum time between trades. |
| `NON_PDT_MODE` | `true` | If `true`, avoids Day Trading rules (PDT). |

### Risk Management (or use `RISK_APPETITE_SCORE`)
| Variable | Default | Description |
|----------|---------|-------------|
| `RISK_APPETITE_SCORE` | - | **Master Override**. Integer 1-9. Sets risk profile (1=Safe, 9=Aggressive). |
| `RISK_PER_TRADE_PERCENT` | `0.015` | % of capital risked per trade (1.5%). |
| `MAX_POSITION_SIZE_PCT` | `0.1` | Max size of a single position as % of portfolio. |
| `MAX_SECTOR_EXPOSURE_PCT` | `0.30` | Max exposure to a single sector. |
| `MAX_DAILY_LOSS_PCT` | `0.02` | Max daily loss before "Kill Switch" (2%). |
| `MAX_DRAWDOWN_PCT` | `0.1` | Max total drawdown allowed (10%). |
| `TRAILING_STOP_ATR_MULTIPLIER`| `5.0` | Multiplier for ATR-based trailing stops. |
| `MAX_POSITION_VALUE_USD` | `5000.0` | Hard cap on position value in USD. |
| `CONSECUTIVE_LOSS_LIMIT` | `3` | Stop trading a symbol after N losses. |

### Trading Strategy Parameters
| Variable | Default | Description |
|----------|---------|-------------|
| `STRATEGY_MODE` | `standard` | Strategy: `standard`, `advanced`, `dynamic`, `trendriding`, `meanreversion`. |
| `EMA_FAST_PERIOD` | `50` | Fast EMA period (Trend Riding). |
| `EMA_SLOW_PERIOD` | `150` | Slow EMA period (Trend Riding). |
| `RSI_PERIOD` | `14` | RSI calculation period. |
| `RSI_THRESHOLD` | `75.0` | RSI Overbought threshold. |
| `TAKE_PROFIT_PCT` | `0.05` | Target profit percentage (5%). |
| `MIN_PROFIT_RATIO` | `2.0` | Minimum Reward/Risk ratio to enter trade. |
| `SPREAD_BPS` | `5.0` | Estimated spread in basis points for cost calculation. |
| `SLIPPAGE_PCT` | `0.001` | Estimated slippage (0.1%). |
| `COMMISSION_PER_SHARE` | `0.001` | Estimated commission per share. |

### Dynamic & Adaptive Mode
| Variable | Default | Description |
|----------|---------|-------------|
| `DYNAMIC_SYMBOL_MODE` | `false` | Enable automatic symbol discovery. |
| `DYNAMIC_SCAN_INTERVAL_MINUTES`| `5` | How often to scan for new "Top Movers". |
| `ADAPTIVE_OPTIMIZATION_ENABLED`| `false` | Enable self-optimization. |
| `MIN_VOLUME_THRESHOLD` | `50000` | Min volume for tradeable assets. |
| `SIGNAL_CONFIRMATION_BARS` | `2` | Number of bars to confirm a signal. |

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
