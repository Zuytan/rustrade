# Rustrade - High-Performance Algorithmic Trading Bot

## 1. System Overview
Rustrade is a sophisticated, high-performance algorithmic trading system written in Rust. Designed for reliability, safe concurrency, and modularity, it leverages the **Tokio** runtime and a **Domain-Driven Design (DDD)** architecture to trade multiple asset classes (Stocks, Crypto) across different brokers (Alpaca, Binance, Mock).

The system prioritizes capital preservation through a "Paranoid" Risk Management engine while employing adaptive strategies that react to changing market regimes (Trending, Ranging, Volatile).

## 2. Core Architecture

### Agent System
The application operates as a mesh of autonomous agents communicating via high-performance channels, managed by a unified **`SystemClient`** facade:
- **Sentinel Agent**: Ingests real-time market data (WebSockets/REST) and normalizes it.
- **Analyst Agent**: The "Brain". Modular architecture (`RegimeHandler`, `PositionLifecycle`, `NewsHandler`) separating regime detection, position management, and news processing. Maintains symbol state and generates trade proposals.
- **Risk Manager**: The "Gatekeeper". Validates every proposal against a strict set of risk rules and portfolio limits.
- **Executor Agent**: Handles order placement, modification, and reconciliation with the broker.
- **Listener Agent**: Monitors news feeds (RSS, Social) and uses NLP to trigger immediate reactions to market-moving events.
- **User Agent**: Manages the UI/Dashboard state and handles user commands.

### Resilience & Safety
- **State Persistence ("No Amnesia")**: Critical state (Daily Loss, High Water Mark) is persisted to SQLite, preventing rule bypass via restarts.
- **Circuit Breakers**:
  - **Global**: Halts trading if Daily Loss or Drawdown limits are breached.
  - **Infrastructure**: Wraps API calls with retry policies and breakers to handle broker outages gracefully.
  - **Panic Mode**: "Blind Liquidation" logic ensures positions can be exited even if price feeds are down.
  - **Order Monitor**: Active tracking of Limit orders with automatic timeout detection and fallback to Market orders ("Cancel & Replace") to ensure execution.

### Circuit Breaker Thresholds

The system enforces three safety limits configured via `CircuitBreakerConfig`:

| Parameter | Default | Description | Configurable Via |
|-----------|---------|-------------|------------------|
| `max_daily_loss_pct` | **2%** | Maximum loss in a single trading session | Risk Score (auto-scaled) |
| `max_drawdown_pct` | **5%** | Maximum decline from equity high water mark | Risk Score (auto-scaled) |
| `consecutive_loss_limit` | **3 trades** | Halt after N consecutive losing trades | Fixed (code-level config) |

**How it works**:
- **Daily Loss**: Calculated as `(current_equity - session_start_equity) / session_start_equity`
- **Drawdown**: Calculated as `(current_equity - equity_high_water_mark) / equity_high_water_mark`
- **Consecutive Losses**: Counter incremented on trade close with negative P&L, reset on winning trade

> [!CAUTION]
> If any limit is breached, **all new trades are blocked** and emergency portfolio liquidation is triggered. Manual intervention required to reset (or automatic reset at session start for daily loss).

**State Persistence**: Circuit breaker state is persisted to SQLite (`risk_state` table) to prevent bypass via application restart.

- **Concurrency**: Deadlock-free design using timeouts on locks and message-passing patterns.

## 3. Trading Intelligence

### Strategy Engine
Rustrade supports a diverse suite of strategies, organized by market approach:
- **Trend Following**: `TrendRiding` (EMA Crossovers), `AdvancedTripleFilter` (SMA + RSI + MACD + ADX), `StatisticalMomentum` (ATR-normalized).
- **Mean Reversion**: `MeanReversion` (Bollinger Bands), `VWAP` (Volume Weighted Average Price), `ZScoreMeanReversion` (Statistical Z-Score).
- **Market Structure**: `SMC` (Smart Money Concepts - Order Blocks, FVGs with Strict Zone Mitigation logic), `Breakout` (Volume/Range).
- **Order Flow**: `OrderFlow` (Institutional footprints via stacked imbalances, Cumulative Delta, HVN support/resistance).
- **Machine Learning**: `MLStrategy` (Random Forest Regressor utilizing advanced statistical features).
- **Adaptive**: `RegimeAdaptive` (Dynamic ensemble that switches strategies and risk profile based on Hurst Exponent and Volatility).
- **Ensemble**: Voting system combining multiple strategies.

### Adaptive Features
- **Regime Adaptation**: The `RegimeAdaptive` mode employs a `RegimeDetector` (using ADX, Variance, Linear Regression) to classify the market as `Trending` (Up/Down), `Ranging`, or `Volatile`. It automatically switches the active strategy (e.g., Trend -> VWAP in range) to match conditions.
- **Dynamic Risk Scaling**: Automatically scales down risk exposure (Risk Score) during adverse regimes (e.g., Flash Crashes).
- **Multi-Timeframe Analysis**: Aggregates 1-minute data into higher timeframes (5m, 15m, 1h, 4h, 1d) to validate trends ("Zoom Out" confirmation).

### Machine Learning Architecture
- **Data Collection**: `DataCollector` agent passively captures FeatureSets and labels them with future returns (1m, 5m, 15m), persisting them to CSV for training.
- **Inference Engine**: `SmartCorePredictor` loads pre-trained Random Forest models to generate real-time trade probabilities.
- **Training Pipeline**: Standalone `train_ml` binary for offline model retraining.

### News & Sentiment
- **NLP Analysis**: Uses local VADER sentiment analysis with financial keyword boosting to classify news headlines (Bullish/Bearish).
- **Macro Sentiment**: Integrates "Fear & Greed Index" to adjust global risk appetite.

## 4. Risk Management System

### Dynamic Risk Profile
- **Risk Appetite Score (1-10)**: A single "Master Knob" that auto-tunes 12+ underlying technical parameters.
  - **Score 1 (Safe)**: Tight stops (1.5x ATR), small size (0.5%), strict trend requirements.
  - **Score 10 (Extreme)**: Loose stops (8x ATR), "All-In" sizing (20%+), aggressive entry.
- **Signal Sensitivity Scaling**: Conservative profiles (1-3) automatically receive more sensitive entry thresholds (50-70% of normal), ensuring they generate trades even in low-volatility markets.
- **Hard Stop Protection**: Per-trade loss limit (`max_loss_per_trade_pct`, default -5%) forces position exit to prevent extreme drawdowns.

### Validation Pipeline
Every trade proposal passes through a Chain of Responsibility validation:
1.  **Buying Power Validator**: Ensures sufficient cash (accounting for open orders).
2.  **Circuit Breaker Validator**: Checks global loss limits.
3.  **PDT Validator**: Prevents Pattern Day Trading violations for small accounts (<$25k).
4.  **Position Size Validator**: Enforces max risk per trade and max position size.
5.  **Sector/Correlation Validator**: Prevents over-exposure to a single sector or highly correlated assets.
6.  **Sentiment Validator**: Blocks aggressive buys during "Extreme Fear".

## 5. User Interface & Experience

### Agentic Desktop UI
Built with `egui` (Native) for low-latency performance, featuring a modular component architecture pattern (MVVM):
- **Dashboard**: Real-time visualization of Portfolio Value, Win Rate (Donut), P&L History (Chart), and Active Positions.
- **Activity Feed**: Live log of system events, trades, and rejected proposals.
- **News Feed**: Real-time stream of analyzed news with sentiment badges.
- **Configuration Panel**:
  - **Simple Mode**: Risk Score slider with **automatic strategy selection** (Risk 1-3→Standard, 4-6→RegimeAdaptive, 7-10→SMC).
  - **Advanced Mode**: Granular control over SMA periods, RSI thresholds, manual strategy override.
- **Internationalization (I18n)**: Full support for English and French, with dynamic language switching.

## 6. Infrastructure & Data

### Connectivity
- **Broker Agnostic**: Seamlessly switches between Alpaca (Stocks/Crypto), Binance (Crypto), and Mock (Paper Trading).
- **Modular Services**: Each broker infrastructure is organized into focused modules:
  - **Binance**: `common.rs`, `market_data.rs`, `execution.rs`, `sector_provider.rs`, `websocket.rs`
  - **Alpaca**: Similar modular structure for consistency
- **Cost Modeling**: Unified `FeeModel` handles commission and slippage calculations for accurate simulation.

### UI Architecture
- **Component-Based**: UI is organized into reusable components:
  - `dashboard_components/` - Portfolio, charts, positions display
  - `settings_components/` - Risk settings, strategy parameters, language selection
- **MVVM Pattern**: ViewModels separate UI rendering from business logic for better testability.

### Data Optimization
- **Smart Caching**: `CandleRepository` caches historical data locally (SQLite). Services use an incremental load strategy to minimize API calls and vastly speed up startup (Warmup).
- **Dynamic Crypto Scanner**: dedicated "Top Movers" scanner for 24/7 crypto markets, automatically discovering new listings via exchange APIs.

## 7. Performance & Verification

- **Simulator & Optimization**:
  - Detailed backtesting engine capable of replaying historical data (including specific crash scenarios) to verify strategy logic and metrics (Alpha, Beta, Sharpe).
  - **Parallel Execution**: Leverages `Rayon` for multi-threaded backtesting, delivering massive speedups on multi-core CPU architectures.
- **Quality Assurance**: 
  - 100% Test Coverage on core logic (Risk, Sizing).
  - CI pipeline enforcing `clippy` (linting) and `fmt` standards.
  - "No Unwraps" policy in production code for stability.

## 8. Server Mode & Observability

### Headless Deployment
Rustrade can run without a GUI for server deployments:
- **Server Binary**: `cargo run --bin server` - Runs the full trading system without UI.
- **UI Optional**: The `ui` feature flag can be disabled for headless builds.

### Push-Based Metrics
The system outputs metrics **outbound only** - no HTTP server, no incoming connections:
- **Structured JSON to stdout**: Periodic output prefixed with `METRICS_JSON:` for log aggregators
- **Configurable interval**: `OBSERVABILITY_INTERVAL` environment variable (default: 60s)

Metrics include:
- Portfolio value, cash, positions count
- Per-symbol position details (quantity, average price, value)
- System status (circuit breaker, uptime, version)

**Security Note**: The system only SENDS data, it never accepts incoming requests.

## 9. Contributor Documentation

The project includes comprehensive documentation for contributors:
- **LICENSE**: MIT License for open-source compliance.
- **CONTRIBUTING.md**: Development setup, code style guidelines, PR process.
- **docs/STRATEGIES.md**: Technical documentation for all 10 trading strategies.
- **README.md**: Badges, architecture diagram, screenshots, and quick start guide.
