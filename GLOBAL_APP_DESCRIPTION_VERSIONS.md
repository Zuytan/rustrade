# Rustrade - Historique des Versions

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
