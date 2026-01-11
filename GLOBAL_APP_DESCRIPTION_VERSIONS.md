# Rustrade - Historique des Versions


## Version 0.66.0 (Janvier 2026) - Benchmark Optimization

- **Signal Sensitivity Scaling**:
  - Added `calculate_signal_sensitivity_factor()` to `RiskAppetite`.
  - Conservative profiles (1-3) now use 50-70% of normal signal thresholds.
  - Balanced profiles (4-6) use 70-90% of thresholds.
  - Fixes issue where Risk-2 and Risk-5 profiles generated zero trades.

- **Breakout Strategy Tuning**:
  - Reduced `lookback_period` from 20 to 10 for faster detection.
  - Reduced `breakout_threshold_pct` from 0.5% to 0.2%.
  - Reduced `volume_multiplier` from 1.3 to 1.1 (more permissive).

- **Hard Stop Manager**:
  - New `HardStopManager` in `risk_management/` for per-trade loss limits.
  - Default `-5%` max loss per trade before forced exit.
  - Prevents extreme drawdowns observed in benchmarks (e.g., -1317%).

- **Enhanced RegimeAdaptive Strategy**:
  - Added hysteresis (≥60% confidence required to switch strategies).
  - Low-volatility Ranging now uses MeanReversion instead of VWAP.
  - High-volatility Ranging continues to use VWAP.

- **Files Modified** (10+): `risk_appetite.rs`, `analyst.rs`, `breakout.rs`, `strategy_factory.rs`, `strategy_selector.rs`, `mod.rs`
- **Files Added** (1): `hard_stop_manager.rs`
- **Verification**: All tests pass (133+).

## Version 0.65.1 (Janvier 2026) - Risk Manager Decomposition

- **Risk Manager Refactoring**:
  - **Circuit Breaker Service**: Extracted circuit breaker logic (Drawdown, Daily Loss, Consecutive Losses) into a dedicated `CircuitBreakerService`.
  - **Order Reconciler**: Extracted order reconciliation and pending order tracking into `OrderReconciler`.
  - **Risk Manager Cleanup**: Simplified `RiskManager` to be a high-level orchestrator delegation to these services.
  - **Verification**: 32 unit tests verified the integrity of the risk logic after refactoring.

## Version 0.65.0 (Janvier 2026) - Code Decomposition & Infrastructure Refactoring

- **Analyst Logic Decomposition**:
  - **TradeEvaluator**: Encapsulation of signal validation and proposal generation logic via `TradeEvaluator` service.
  - **SignalProcessor**: Central execution of strategy signals and filtering (RSI, Trends) via `SignalProcessor`.
  - **WarmupService**: Isolated warm-up logic for cleaner startup sequences via `WarmupService`.
  - **Result**: Significant reduction in complexity for the core `Analyst` agent (reduced by ~1300 lines).

- **Infrastructure Modularization**:
  - **Organized Directory Structure**: Grouped broker implementations into `infrastructure/alpaca`, `infrastructure/binance`, and `infrastructure/oanda`.
  - **Core Shared Components**: Centralized common utilities (circuit breakers, event bus) in `infrastructure/core`.
  - **Clean Exports**: Updated `mod.rs` files for cleaner API surfaces and better encapsulation.

- **Code Quality & Hygiene**:
  - **Dead Code Removal**: Pruned unused imports, commented-out blocks, and legacy TODOs/FIXMEs.
  - **Documentation Cleanup**: Removed outdated comments from core files (`config.rs`, `system.rs`, `analyst.rs`).

- **Verification**:
  - Full regression suite passed (250+ tests).
  - Zero `clippy` warnings.
  - Clean `cargo check`.

## Version 0.64.0 (Janvier 2026) - Dependency Upgrade & API Modernization

- **Major Dependency Updates**:
  - **egui/eframe**: 0.31.0 → 0.33.3 (Breaking API changes fixed)
  - **egui_plot**: 0.31.0 → 0.34.0 (New constructor signatures)
  - **reqwest**: 0.12 → 0.13 (Added `query` feature flag)
  - **reqwest-middleware**: 0.3 → 0.5 (Breaking change: `.query()` removed)
  - **reqwest-retry**: 0.5 → 0.9
  - **prometheus**: 0.13 → 0.14
  - **crossbeam-channel**: 0.5.14 → 0.5.15
  - **serde_json**, **url**, **rust_decimal_macros** updated

- **Infrastructure Improvements**:
  - **build_url_with_query()**: New helper function to construct URLs with query parameters since `reqwest-middleware` 0.5 removed `.query()` method.
  - Updated 9 HTTP calls in `alpaca.rs` and `binance.rs`.
  - Fixed `egui_plot` constructor calls in `dashboard.rs`.

- **Verification**: 25 tests passing, zero clippy warnings, clean build.
- **Files Modified** (5): `Cargo.toml`, `http_client_factory.rs`, `alpaca.rs`, `binance.rs`, `dashboard.rs`

## Version 0.62.0 (Janvier 2026) - Server Mode & Observability
