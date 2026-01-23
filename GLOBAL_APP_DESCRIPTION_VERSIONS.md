# Rustrade - Historique des Versions

## Version 0.84.0 - Audit Remediation: Performance, Bias & Stability (January 2026)

### Critical Performance Fix
- **Portfolio Caching**: Eliminated 200-500ms API latency from trade execution path.
  - Implemented `Arc<RwLock<Portfolio>>` cache in `AlpacaExecutionService`.
  - Background polling task updates cache every 1 second.
  - `get_portfolio()` now returns instantly (<1µs) instead of blocking on HTTP calls.
  - **Impact**: Significantly reduced slippage risk during volatile market conditions.

### Strategy Robustness (Look-Ahead Bias Fix)
- **SMC Strategy**: Fixed critical look-ahead bias in Fair Value Gap (FVG) detection.
  - Changed entry validation from intra-candle `Low/High` to `Close` price.
  - Ensures backtest results are reproducible in live trading.
  - Updated all SMC unit tests to reflect bias-free logic.

### Code Stability & Quality
- **ADX Indicator**: Restored and corrected `ManualAdx` implementation.
  - Fixed Wilder's smoothing initialization (accumulate first N values, then smooth).
  - Removed dependency on non-existent `AverageDirectionalIndex` from `ta` crate.
- **Error Handling**: Eliminated duplicate imports and unused code warnings.
- **Zero Clippy Warnings**: All 350 tests passing with strict linting enabled.

### Configuration & UX
- **Flexible Trading Hours**: Added `--session-start` and `--session-end` CLI arguments to `optimize.rs`.
  - Supports 24/7 crypto markets and international trading sessions.
  - Default: US market hours (14:30-21:00 UTC).

### Audit Compliance
- Addressed all critical and warning-level findings from quantitative algorithm audit.
- Score improved from 7/10 to production-ready status.

## Version 0.83.0 - Decimal Precision & Order Resilience (January 2026)

### Type Safety Migration
- **Decimal Precision**: Complete migration of all financial calculations (Prices, Volumes, Indicators) from `f64` to `rust_decimal::Decimal`.
  - Eliminates floating-point errors in strategy logic and P&L calculations.
  - Updated all 5 major strategies (VWAP, SMC, Breakout, Momentum, OrderFlow) to use `Decimal` arithmetic.
  - Updated Database repositories to store/retrieve accurate decimal values.

### Order Resilience
- **Order Monitoring**: Implemented a dedicated `OrderMonitor` system.
  - **Timeout Detection**: Automatically detects Limit orders that remain unfilled beyond `limit_timeout_ms`.
  - **Cancel & Replace**: Triggers automatic cancellation of stale Limit orders and replaces them with Market orders to guarantee exit/entry.
  - **Executor Integration**: Seamlessly integrated into the Executor's main loop for zero-overhead monitoring.

### Quality & Verification
- **100% Test Pass Rate**: Verified all 16 integration scenarios and hundreds of unit tests with the new Decimal types.
- **Robustness**: Fixed various compilation errors and type mismatches across the entire codebase.

## Version 0.82.0 - Critical Volume Data Fix (January 2026)

### Data Ingestion Fix
- **Volume Propagation**: Resolved a critical issue where trade volume was being discarded and replaced with tick counts.
- **Domain Model**: Updated `MarketEvent::Quote` to include a `quantity: Decimal` field.
- **Infrastructure**: Updated WebSocket handlers (Binance, Alpaca) to correctly parse and propagate trade size.
- **Application**: Updated `CandleAggregator` to accumulate real trade volume.

### Impact
- **Strategy Accuracy**: Volume-based strategies (VWAP, Order Flow, SMC) now receive accurate data.
- **Backwards Compatibility**: Breaking change for `MarketEvent::Quote` consumers resolved across all agents.

## Version 0.81.0 - Audit Recommendations Implementation (January 2026)
