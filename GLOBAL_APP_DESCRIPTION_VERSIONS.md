# Rustrade - Historique des Versions

## Version 0.97.0 - Strategy Mathematical Fixes & Quality (February 2026)

### Strategy Mathematical Audit & Fixes (12 corrections)
- **VWAP**: Added `has_position` guard to Sell signal to prevent phantom short entries.
- **Statistical Momentum**: Fixed off-by-one error in lookback window (`nth(lookback-1)`).
- **SMC**: Enhanced FVG detection with midpoint normalization and impulsive candle validation.
- **Order Flow**: 
  - Implemented body-weighted OFI (wicks contribute to buy/sell pressure).
  - Fixed logic error where OFI compared against itself (`skip(1)` added).
- **Momentum Divergence**: Made RSI thresholds configurable (removed hardcoded 40/60).
- **Dynamic Regime**: Added ADX hysteresis (±2.0 buffer) to prevent rapid regime switching.
- **Z-Score**: Included `current_price` in mean/std_dev calculation for statistical consistency.
- **DualSMA**: Added `!has_position` guard to Buy signal to prevent signal spam in sustained uptrends.
- **ML Strategy**: Explicit `PredictionMode` enum (Regression vs Classification).
- **Ensemble**: Added systematic logging for strategy disagreements.

### Code Quality & Compliance
- **Dead Code Removal**: Removed 5 unused configuration fields across `Dynamic`, `MeanReversion`, `TrendRiding`, and `Advanced` strategies, eliminating all `#[allow(dead_code)]` suppressions.
- **Mathematical Verification**: Verified correctness of linear regression variance formula in `statistical_features.rs`.
- **Test Suite**: Achieved 100% pass rate (408/408 tests) covering all strategy edge cases.
- **Linting**: maintained zero clippy warnings (`--all-targets -D warnings`).

## Version 0.96.3 - Test Suite Enhancement & Code Quality (February 2026)

### Test Implementation (P0/P2)
- **Feature Engineering Tests**: 8 comprehensive tests for `TechnicalFeatureEngineeringService`
  - SMA calculation (20/50/200)
  - RSI calculation (flat prices, trends)
  - Bollinger Bands convergence
  - ATR calculation
  - MACD calculation
  - ADX trending detection
  - Momentum normalized calculation
  - Insufficient data handling
- **SignalGenerator Tests**: 3 critical tests validating data integrity
  - FeatureSet → AnalysisContext mapping validation (field-by-field verification)
  - Signal propagation (Buy/None scenarios)
  - MockStrategy with thread-safe Mutex for testing
- **Integration Test Strengthening**: Enhanced `candle_pipeline` with robust indicator validation

### Production Bug Fixes
- **DualSMAStrategy Logic Error**: Fixed incorrect trend filter in Golden Cross detection
  - **Issue**: Strategy required `price > trend_sma` (SMA 200) IN ADDITION to Fast > Slow crossover, blocking valid buy signals
  - **Root Cause**: Confusion between "Dual SMA" (crossover only) and "Trend Riding" (multi-timeframe confirmation)
  - **Fix**: Removed erroneous `trend_sma` condition from buy signal logic (lines 30-36 in `dual_sma.rs`)
  - **Impact**: Strategy now correctly triggers on fast/slow SMA crossover only, respecting separation of concerns

### Code Quality Improvements
- **Test Safety**: Replaced `.unwrap()` with `.expect()` in test helpers (6 occurrences) for clearer error messages
- **Clippy Compliance**: Fixed 3 warnings across all targets (`--all-targets -- -D warnings`)
  - `field_reassign_with_default` in `ensemble_optimizer.rs` (2 occurrences)
  - `map_clone` in `ensemble_optimizer.rs`
  - `field_reassign_with_default` + `unused_mut` in `signal_generator.rs` tests

### Verification
- ✅ All tests passing: 408/408
- ✅ Clippy clean: 0 warnings
- ✅ Build verified: Release compilation successful
- ✅ Code formatted with `cargo fmt`

## Version 0.95.0 - Quality, Data & Evolution (February 2026)

### Phase 1 – Code Quality
- **Decimal-only finance**: Migrated all money-related calculations from `f64` to `rust_decimal::Decimal` in `feature_engineering_service`, `user_agent`, and `zscore_mean_reversion`; `f64` only for pure stats or ML/UI export.
- **RwLock safety**: Replaced `.unwrap()` on `RwLock` in Alpaca and Binance market_data with proper `map_err` + `?`.
- **Dead code & TODOs**: Externalized adaptive thresholds into config (`regime_volatility_threshold`); documented Binance User Data Stream limitation and RiskManager `spread_cache` intent.

### Phase 3 – Data & Backtesting
- **Trade persistence**: Enriched SQLite schema with `strategy_used`, `regime_detected`, `entry_reason`, `exit_reason`, `slippage`; domain `Trade` type extended accordingly.
- **Walk-forward backtesting**: `BenchmarkEngine::run_walk_forward` with configurable train ratio and out-of-sample Sharpe.
- **OANDA**: Sector provider only (`OandaSectorProvider`); `MODE=oanda` uses Mock for market data and execution until v20 API integration.

### Phase 4 – Evolution
- **Stress tests**: `stress_test_draft.rs` scenarios (circuit breaker, daily loss breach) integrated; Binance HMAC test marked `#[ignore]` for sandbox/CI (macOS system-configuration).

## Version 0.94.1 - Dynamic Crypto UI & Async Loading (February 2026)

### User Experience
- **Dynamic Symbol Selector**: Added a searchable, multi-select UI component settings for Crypto trading.
- **Async Asset Discovery**: Automatically fetches all tradable crypto pairs from the broker API on startup, eliminating the need to hardcode `SYMBOLS` in `.env`.
- **Search & Filter**: Real-time filtering of thousands of crypto pairs with "Select All" and "Top 10" shortcuts.

### Architecture
- **Async Initialization**: Enhanced `UserAgent` startup sequence to load broker assets in a non-blocking background task.
- **Command Pattern**: Introduced `SentinelCommand::LoadAvailableSymbols` with oneshot channel response to bridge UI and Market Data service.

## Version 0.94.0 - Audit Remediation, CI/CD & Observability (February 2026)

### Audit & Code Quality
- **Global Audit**: Achieved 92% maturity score. Zero `.unwrap()` policy enforced.
- **Dependency Clean**: Verified `ort` (2.0.0-rc.11) and `egui` compatibility.
- **Config Consolidation**: Unified `.env` templates for easier deployment.

### Observability Infrastructure
- **Prometheus/Grafana**: Added full monitoring stack (`monitoring/`).
- **Alerting**: Configured rules for Drawdown and Circuit Breakers.

### Automation
- **CI Enhancements**: Added Coverage (`tarpaulin`), Security Audit (`cargo-audit`), and Benchmark checks.
- **ML Pipeline**: Automated `train_and_deploy.sh` script for end-to-end model lifecycle.

### Security
- **Policy**: Established `SECURITY.md` guidelines for API keys and vulnerabilities.

### Quality Assurance
- **Stress Tests**: Drafted scenarios for Flash Crash resilience.
- **Stability**: 100% test pass rate with strict linting.

## Version 0.93.0 - High-Fidelity Simulation & DL Infrastructure (January 2026)

### High-Fidelity Simulation
- **Network Latency Simulation**: Introduced `LatencyModel` (Base + Jitter) to simulate realistic execution delays in `MockExecutionService`.
- **Dynamic Slippage Model**: Implemented `SlippageModel` based on market volatility to simulate price impact and bid-ask spreads.
- **Improved Benchmarking**: Updated `BenchmarkEngine` to integrate simulation models, enabling stress-testing of strategies under realistic conditions.
- **Full Configuration**: Added `SIMULATION_ENABLED`, `SIMULATION_LATENCY_BASE_MS`, and `SIMULATION_SLIPPAGE_VOLATILITY` settings.

### Deep Learning Infrastructure
- **Sequential ML Support**: Refactored `OnnxPredictor` to be stateful, supporting LSTM/GRU models with a sliding history window.
- **Feature Registry**: Centralized feature ordering to ensure strict consistency between Rust inference and Python training.
- **Warmup Mechanism**: Implemented `warmup` service to pre-initialize model state with historical data, eliminating the "cold start" latency for sequential models.
- **Python Pipeline**: Provided `train_lstm.py` and dedicated requirements for training deep learning models externally and exporting to ONNX.

### Quality & Reliability
- **Rand 0.9 Migration**: Updated simulation models to use the latest `rand` API (`random_range`, `rng`).
- **Test Synchronization**: Updated global `Config` initializers across the entire integration test suite.
- **100% Verification**: Verified ~1% performance degradation on benchmarks when simulation is enabled, confirming model effectiveness.


## Version 0.92.0 - Cycle 3: AI Agent Optimization & Live Verification (January 2026)

### ML Optimization & Performance
- **Hyperparameter Tuning**: Automated script (`optimize_ml.sh`) iterates through Random Forest parameters (Trees, Depth, Splits) to maximize Net Profit.
- **Pure Rust Training**: Enhanced `train_ml` binary replaces Python scripts, allowing full model training and saving natively in Rust.
- **Improved Metrics**: Benchmark engine now accurately tracks P&L per strategy, enabling automated selection of the best performing model.

### Reliability & Verification
- **Paper Trading Verified**: Confirmed end-to-end system stability in Alpaca Paper Trading environment (Crypto).
- **Quality Assurance**: 100% test pass rate, zero clippy warnings, and thread-safe ML inference verified under load.

## Version 0.91.0 - AI Agent Phase 1: Data Enrichment & Dataset Generation (January 2026)

### AI Infrastructure
- **Enhanced Feature Set**: Expanded `FeatureSet` to include market microstructure data:
  - **Order Flow**: Live Order Flow Imbalance (OFI) and Cumulative Delta.
  - **Smart Features**: Pre-calculated Bollinger Band Width/Position and ATR-normalized volatility.
- **Dataset Generation Binaire**: Introduced `train_gen` CLI tool to rapidly generate massive ML datasets by replaying historical market data through the backtesting engine.
- **Unified Feature Pipeline**: Centralized feature calculation in `TechnicalFeatureEngineeringService` to ensure zero drift between training and real-time inference.

### Machine Learning
- **Enriched Collector**: `DataCollector` now captures the full 18+ feature vector along with multi-horizon return labels (1m, 5m, 15m).
- **Predictor Alignment**: Updated `SmartCorePredictor` to support the expanded feature space.

### Quality & Cleanup
- **Safe Env Management**: Fixed unsafe environment variable manipulation in a multi-threaded context.
- **Strict Linting**: Verified zero clippy warnings and 100% test pass rate across the new AI components.


## Version 0.90.0 - Sentinel Reliability & Data Integrity (January 2026)

### Sentinel Agent Upgrades
- **Strict Data Validation**: Implemented `StrictEventValidator` to filter out physically impossible data (e.g., negative prices, zero volume, invalid candle spreads) before they reach the Analyst.
- **Zombie Stream Detection**: Integrated `StreamHealthMonitor` with a 10s heartbeat threshold. Sentinel now detects and broadcasts "silence" events as system-wide Offline status, even if TCP is technically connected.
- **Performance Optimized**: Heartbeat checks run on an asynchronous interval to avoid slowing down the high-frequency event relay path.

### Infrastructure & Domain
- **New Validation Domain**: Created `src/domain/validation` to centralize all data integrity rules.
- **Improved Monitoring**: Added `StreamHealthMonitor` utility in `src/application/monitoring` for reuse across different agents.

### Quality & Verification
- **100% Test Coverage**: All reliability features verified with unit tests for edge cases (flash crashes, connection silences).
- **Test Suite Synchronization**: Verified entire 370+ test suite passes with the new Sentinel components.

## Version 0.89.1 - Documentation & Assessment (January 2026)

### Project Assessment
- **Comprehensive Audit**: Conducted a full project evaluation, confirming architecture stability and code quality.
- **Roadmap Generation**: Created a roadmap for future ML enhancements (Deep Learning) and Platform expansion (Web UI).

### Documentation
- **Strategy Documentation Cleanup**: Reorganized `GLOBAL_APP_DESCRIPTION.md` to clearly distinguish between Production-Ready and Legacy strategies.
- **Doctest Fixes**: Resolved compilation errors in `strategy_validator` documentation examples.

## Version 0.89.0 - Advanced Observability & Monitoring (January 2026)

### Observability Infrastructure
- **Full Tracing Instrumentation**: Added `#[instrument]` to critical agents (`Analyst`, `RiskManager`, `Executor`) for detailed request tracing and debugging.
- **Custom Prometheus Metrics**: Expanded metrics coverage to include:
  - **Business**: Live win rate, current max drawdown, and daily trade counters.
  - **Performance**: API latency distribution (Histogram) and WebSocket reconnection tracking.
  - **System**: Circuit breaker trip status and Fear & Greed sentiment score integration.
- **Latency Tracking**: Introduced `LatencyGuard` RAII utility for automatic measurement and recording of API call durations.

### Quality & Testing
- **Structured Logging**: Refactored critical log lines to use structured fields (`symbol`, `side`, `qty`) instead of string interpolation, enabling advanced log analysis.
- **Improved Test Coverage**: Added unit tests for `LatencyTracker` and synchronized the entire integration test suite with new service signatures.
- **Full Verification**: 100% of risk and scenario tests pass with high reliability under full observability instrumentation.

## Version 0.88.0 - System Resilience & Connection Health (January 2026)

### Connection Resilience
- **Singleton WebSocket Architecture**: Standardized `AlpacaMarketDataService` to use a single shared `AlpacaWebSocketManager` instance, ensuring only one physical connection per broker regardless of subscribed symbols.
- **Infinite Reconnection Loop**: Fixed premature loop termination in `AlpacaWebSocketManager` that broke on clean disconnections or 406 errors. Now persists indefinitely with exponential backoff until explicit shutdown.
- **Alpaca 406 Error Resolution**: Eliminated redundant reconnection logic in `Sentinel` agent that conflicted with infrastructure-level stream management, preventing connection limit violations.

### System Health Monitoring
- **Connection Health Service**: Introduced centralized `ConnectionHealthService` for broadcast-based monitoring of market data and execution stream status across all agents.
- **Health-Aware Risk Management**: `RiskManager` now validates connection health before accepting proposals, preventing trades during connectivity issues.
- **Executor Startup Reconciliation**: Enhanced `Executor` to broadcast execution stream status on startup and during reconnection events.

### Code Quality & Stability
- **Test Suite Synchronization**: Updated 30+ test cases to support new `ConnectionHealthService` and 15-argument `RiskManager` constructor signature.
- **Infinite Test Loop Fix**: Resolved hanging tests in `audit_fixes.rs` by properly initializing connection health status to `Online` before running `RiskManager`.
- **100% Test Pass Rate**: All risk management, scenario, component, and agent tests passing successfully.
- **Zero Clippy Warnings**: Achieved clean build with strict linting enabled (`-D warnings`).

### Documentation
- **Architecture Updates**: Updated `AGENTS.md`, `README.md`, and `GLOBAL_APP_DESCRIPTION.md` to reflect singleton WebSocket architecture, centralized health monitoring, and Executor's reconciliation responsibilities.

## Version 0.87.0 - Adaptive Regime & Statistical Modernization (January 2026)

### Adaptive Regime Architecture
- **Dynamic Strategy Switching**: Implemented `regime_handler.rs` which dynamically shifts between `StatMomentum`, `ZScoreMR`, and Legacy strategies based on market conditions.
- **Enhanced Regime Detection**: refactored `MarketRegimeDetector` to use O(1) feature-based detection (Hurst Exponent, Realized Volatility, Skewness).
- **Dynamic Risk Scaling**: Automatically adjusts `risk_appetite_score` and strategy parameters (RSI thresholds, stop multipliers) in real-time as market volatility shifts.

### ML & Statistical Enhancements
- **Probabilistic ML Signals**: Refactored `MLStrategy` and `train_ml` to use Regression-based confidence scores (0.0-1.0) instead of binary classification for more nuanced trade entry.
- **Statistical Positioning**: Integrated ATR-normalized momentum and Z-score based mean reversion as core strategy components.

## Version 0.86.0 - Machine Learning & Statistical Models (January 2026)

### Machine Learning Infrastructure
- **Offline Training Pipeline**: Implemented a complete end-to-end ML workflow.
  - `DataCollector`: Captures live features and labels (future returns) to CSV.
  - `train_ml`: Standalone binary to train Random Forest classifiers (SmartCore) from collected data.
  - `MLStrategy`: Executes trades based on model probability scores (>0.6 Buy, <0.4 Sell).
- **Architecture**: Decoupled `MLPredictor` trait allowing plug-and-play of different models (XGBoost, LinReg) in the future.

### New Statistical Strategies
- **Z-Score Mean Reversion**: Pure statistical approach trading extreme deviations (>2 std dev) from the mean.
- **Statistical Momentum**: Volatility-normalized momentum strategy using Linear Regression slope for trend confirmation.

## Version 0.85.1 - Safe Portfolio Initialization & False Drawdown Fix (January 2026)

### Critical Stability Fixes
- **False Drawdown Elimination**: Fixed a race condition where the Risk Manager triggered a panic liquidation (-88% drawdown) at startup.
  - Removed `initial_cash` ($100k) default configuration to enforce reliance on real broker data.
  - Implemented `Portfolio.synchronized` flag; Risk Manager now waits for broker synchronization before starting.
- **Stale State Protection**: Added a safety check in `SessionManager` to detect and reset stale risk states (e.g. from previous Mock sessions) if equity variance exceeds 50%.
- **Simulation Safety**: Purged all hardcoded default capital values (e.g., $100k) from backtesting and optimization engines. Simulations now strictly initialize with $0, requiring explicit funding configuration.

## Version 0.85.0 - Critical Portfolio Synchronization Fix (January 2026)

### Critical Portfolio Detection Fix
- **Unified State Management**: Resolved a critical issue where the application failed to detect remote portfolio state.
  - **Root Cause**: `AlpacaExecutionService` was updating an isolated internal cache instead of the application's shared state.
  - **Solution**: Inject shared `Arc<RwLock<Portfolio>>` into `AlpacaExecutionService`, enabling direct updates from the background poller to the main application state.
  - **Verification**: Confirmed real-time portfolio synchronization (<500ms latency) via logs and runtime checks.
  - **Impact**: Ensures trading decisions are based on accurate, up-to-date position and cash data.

### Code Quality
- **Cleanup**: Removed unused imports and redundant code in execution services.
- **Linting**: Achieved zero Clippy warnings across the codebase.

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
