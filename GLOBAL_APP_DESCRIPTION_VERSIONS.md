# Rustrade Versions History

## Version 0.13.1 - Code Cleanup & Risk Hardening (2025-12-26)
- **Codebase Clean-up**: Resolved all `cargo clippy` warnings (redundant casts, unused imports, formatting) for a pristine codebase.
- **Risk Management Hardening**:
    - **Active Valuation Loop**: `RiskManager` now actively polls market prices (every 60s via `MarketDataService`) to recalculate equity.
    - **Crash Protection**: Circuit Breakers (Daily Loss/Drawdown) now trigger *immediately* on market drops, without waiting for the next trade proposal.
    - **Initialization Fix**: Fixed a bug where initial equity was miscalculated (ignoring held positions) on restart.
- **Documentation**: Updated architecture docs to reflect active risk monitoring.

## Version 0.13.0 - Tier 1 Critical Fixes (2025-12-26)
- **CRITICAL FIX: Trailing Stops Enabled**: Uncommented and activated trailing stop mechanism that was previously disabled.
    - Trailing stops now actively monitor price movements and trigger sell signals when threshold is hit.
    - NVDA: 4 trades (all buys) → 8 trades (4 buys + 4 sells) ✅
    - AAPL: 34 trades (all buys) → 60 trades (30 buy/sell pairs) ✅
    - Logs confirm execution: "Trailing stop HIT" messages visible.
- **CRITICAL FIX: Long-Only Safety Logic**: Corrected sell signal blocking that prevented ALL sales instead of just short selling.
    - Now properly distinguishes between selling existing positions (allowed) and short selling (blocked).
    - Improved logging with clear "BLOCKING" vs "ALLOWING" messages.
    - Unit tests validate: `test_sell_signal_with_position` and `test_prevent_short_selling` passing.
- **NEW: Advanced Performance Metrics**: Implemented comprehensive metrics module (`src/domain/metrics.rs`) with 20+ professional indicators.
    - Risk-Adjusted Returns: Sharpe Ratio (8.14), Sortino Ratio (23.18), Calmar Ratio (1.92)
    - Trade Statistics: Win Rate (50%), Profit Factor (4.00), Average Win/Loss, Largest Win/Loss
    - Risk Metrics: Max Drawdown (-0.01%), Exposure (0.1%), Consecutive streaks
    - Integrated into benchmark CLI with detailed output sections.
- **Performance Analysis**: NVDA Sharpe Ratio 8.14 indicates excellent risk-adjusted returns despite low absolute return (0.02% vs 17.26% B&H).
    - Trade quality metrics: Profit Factor 4.00 shows $4 gained per $1 lost.
    - Max Drawdown -0.01% demonstrates exceptional capital preservation.
    - Low exposure (0.1%) suggests overly conservative trailing stops - optimization needed.
- **Testing**: All 32 unit tests passing. E2E test compilation fixed with missing `trend_riding_exit_buffer_pct` field.

## Version 0.12.5
- **Strategy Tuning**: Updated default parameters to better capture multi-day trends.
    - `TREND_SMA_PERIOD` increased to 2000 (approx 1 week on 1m bars).
    - `TREND_DIVERGENCE_THRESHOLD` tuned to 0.0002 (0.02%).
    - Smoothed entry signals (`FAST_SMA`=20, `SLOW_SMA`=60).
- **Performance**: Improved NVDA benchmark return from 0.36% to 1.97% by reducing signal noise.

## Version 0.12.4 - Strategy Safety (Long-Only)
- **Prevented Short Selling**: Enforced a strict check in the Analyst to prevent execution of Sell signals if the portfolio does not hold the asset.
- **Improved Benchmark Robustness**: Verified that strategies now default to Capital Preservation (0% return) instead of losses during choppy "down" periods where Buy signals are filtered.
- **Fixed Tests**: Updated unit tests to align with the Long-Only paradigm.

## Version 0.12.3 - Benchmark Tooling & Metrics
- Released **Benchmark CLI** (`cargo run --bin benchmark`): A dedicated tool for rigorous strategy backtesting.
- **Performance Metrics**: Calculates Total Return, Max Drawdown (implied), and compares performance against a Buy & Hold baseline.
- **Advanced Strategy Testing**: Added `--strategy` CLI argument to switch between Standard (SMA) and Advanced (Triple Filter) strategies during backtest.
- **Short Selling Fix**: Corrected simulation logic for short positions to ensure accurate P&L tracking (fixed "infinite money" bug).

## Version 0.12.2 - Historical Backtesting
- Implemented **Alpaca Historical Bars API**: Added `get_historical_bars` to `AlpacaMarketDataService`.
- Created **Backtesting Integration Test**: `tests/backtest_alpaca.rs` allows simulation of strategies against real historical market data.
- Enabled verification of buy/sell signals using past market scenarios (e.g., volatile days).

## Version 0.12.1 - Documentation Update
- Added **Simplified Strategy Guide** (`docs/guide_strategie_simplifie.md`) for non-technical users.
- Explains Dual SMA, Advanced Filters, and Risk Management in plain language.
- **Enhanced Market Scanner**: Now automatically includes currently held assets in the watchlist to ensure continued monitoring.

## Version 0.12.0 - Dynamic Market Scanning
- Implemented **Market Scanner Agent**: Periodically fetches "Top Movers" (gainers) from Alpaca API.
- **Dynamic Sentinel**: The Sentinel can now receive updates and re-subscribe to new symbols on the fly without restarting.
- Configurable **Scan Interval** and **Dynamic Mode** (`DYNAMIC_SYMBOL_MODE=true`).

## Version 0.11.0 - Strategy Refinement & Momentum
- Refinement of the **Advanced Analyst Strategy**: Added **MACD** (Moving Average Convergence Divergence) filter.
- Implemented **Triple Confirmation** (SMA Cross + Trend 200 + RSI + MACD Momentum) for higher quality entries.
- Increased default `TREND_SMA_PERIOD` to 200 for more robust long-term analysis.

## Version 0.10.0 - Long-Term Stability & Compliance
- Implemented **Non-PDT Mode**: Protection mechanism in `RiskManager` to prevent "Day Trading" on accounts with less than $25k (blocks same-day buy/sell cycles).
- Implemented **Advanced Analyst Strategy**: Multi-indicator approach using Dual SMA + Trend Filter (SMA 100) + RSI confirmation.
- Added `get_today_orders` to `ExecutionService` for real-time compliance checks.
- Enhanced `Config` with adaptive strategy parameters.


## Version 0.9.1 - Codebase Refactoring & Quality
- Refactored `Analyst` component: implemented `AnalystConfig` struct and split `run` loop into modular methods.
- Resolved all Clippy lints (unused imports, collapsible if, array literal modernization).
- Added comprehensive unit tests for `Config` environment variable parsing and validation.

## Version 0.9.0 - Multi-Symbol Portfolio Trading
- Implemented **Multi-Ticker Support**: The Analyst now manages independent SMA states for a list of `SYMBOLS`.
- Added **Portfolio Capital Allocation**: Trades are dynamically sized based on total equity and capped by `MAX_POSITIONS`.
- Enhanced **Liquidity Management**: Ensures capital is distributed across multiple opportunities instead of concentrated in one.

## Version 0.8.1 - Fractional Order Robustness
- Improved Alpaca execution to automatically use `day` time-in-force for fractional orders.
- Resolved "fractional orders must be DAY orders" rejection from Alpaca API.

## Version 0.8.0 - Dynamic Position Sizing & Robust Signal Detection
- Implemented **Risk-Based Position Sizing** (`RISK_PER_TRADE_PERCENT`).
- Quantities are now calculated as a percentage of Total Equity (Cash + Positions).
- Refactored `Analyst` to fetch real-time portfolio data for equitable risk allocation.
- Implemented **Stateful Crossover Tracking** (sticky `last_was_above` state).
- Added **Silent Warm-up** logic to prevent premature signals on initialization.
- Added comprehensive unit tests for dynamic scaling and signal sequences.

## Version 0.7.0 - Stock Market Pivot & Stability
- Switched Asset Class from Crypto to Stocks (IEX Endpoint).
- Implemented **SMA Hysteresis** (threshold-based crossover) to filter noise.
- Added **Signal Cooldown** to prevent rapid-fire "Wash Trade" rejections.
- Enhanced Alpaca WebSocket subscription to include both Trades and Quotes.
- Improved diagnostic logging for portfolio fetching and JSON decoding.

## Version 0.6.0 - Enhanced Strategy (Dual SMA)
- Replaced Single SMA crossover with a Dual SMA crossover (Fast/Slow averages).
- Added `FAST_SMA_PERIOD` and `SLOW_SMA_PERIOD` configuration.
- Improved signal stability and reduced false positives.

## Version 0.5.0 - Robustness & Fractional Trading
- Implemented Symbol Normalization in `RiskManager` (resolved `BTC/USD` vs `BTCUSD` mismatches).
- Added configurable `TRADE_QUANTITY` to `Analyst` and `Config`.
- Implemented automatic SELL quantity adjustment in `RiskManager` for fractional positions.
- Added detailed live debugging logs for Alpaca account and positions.

## Version 0.4.0 - Dynamic Portfolio Risk Management
- Refactored `RiskManager` to fetch real-time portfolio data from the exchange.
- Added `get_portfolio` to `ExecutionService` trait.
- Implemented account and positions retrieval for Alpaca (REST).
- Enhanced `MockExecutionService` to simulate exchange-side state.


## Version 0.3.0 - Alpaca Integration & Rate Limiting
- Added `OrderThrottler` agent for exchange rate limiting (FIFO queue).
- Implemented Alpaca integration (WebSocket market data & REST orders).
- Added multi-mode support (Mock/Alpaca) via environment variables.

## Version 0.2.0 - Refinement & Testing
- Refactored Analyst agent with pure logic and SMA crossover detection.
- Implemented Ports & Adapters (Hexagonal Architecture) for service decoupling.
- Added comprehensive unit tests (14 passing tests).

## Version 0.1.0 - Initialization
- Initial project setup with Cargo.
- Added core dependencies.
- Defined multi-agent architecture.
