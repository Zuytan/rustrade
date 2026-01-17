# Rustrade - Historique des Versions

## Version 0.78.0 - Optimal Parameters per Risk Level (January 2026)

### New Features
- **Optimal Parameters Discovery**: New hybrid system for discovering and applying optimal strategy parameters by risk level
  - **CLI Tool**: Added `optimize discover-optimal` subcommand that runs grid search for Conservative/Balanced/Aggressive profiles
  - **Risk-Tailored Grids**: Each risk profile uses distinct parameter ranges (conservative = tighter stops, aggressive = wider ranges)
  - **Persistence**: Results saved to `~/.rustrade/optimal_parameters.json` with optimization metadata
  - **One-Click Application**: "Apply Optimal Settings" button in Settings UI when optimal parameters are available
  - **Metadata Display**: Shows optimization date, symbol used, Sharpe ratio, return, and win rate

### Domain Model
- **OptimalParameters**: New value object storing discovered parameters with performance metrics
- **OptimalParametersSet**: Collection type with upsert/get operations per risk profile

### Infrastructure
- **OptimalParametersPersistence**: Handles atomic save/load to disk with proper error handling

### Code Quality
- 7 new unit tests for domain model and persistence layer
- All fmt, clippy checks pass ✅

## Version 0.77.0 - Optimize Binary Refactoring (January 2026)

### Refactoring
- **Modern CLI Pattern**: Refactored `optimize` binary following the same pattern as `benchmark`
  - Migrated from manual argument parsing to `clap` with `Parser` and `Subcommand`
  - Added subcommands: `run` (single symbol) and `batch` (multiple symbols)
  - Reduced main binary from 303 lines to ~205 lines (-32%)
- **Code Organization**: Extracted logic into reusable modules
  - `OptimizeEngine` in `src/application/optimization/engine.rs` - encapsulates service setup and optimization execution
  - `OptimizeReporter` in `src/application/optimization/reporting.rs` - handles console output and JSON export
- **Safety Improvements**: Removed all `.unwrap()` calls in production code
  - Replaced with proper error handling using `anyhow::Context`
  - All date parsing now returns descriptive errors

### Code Quality
- All fmt, clippy checks pass ✅
- Binary help displays correctly with `run` and `batch` subcommands
- Optimizer unit tests passing (2/2)

## Version 0.76.0 - Dynamic Strategy Selection (January 2026)

### New Features
- **Risk-Based Strategy Selection**: Simple Mode now automatically selects the optimal trading strategy based on the user's risk score
  - **Risk 1-3 (Conservative)**: `Standard` strategy - Safe, with ADX filters to avoid choppy markets
  - **Risk 4-6 (Balanced)**: `RegimeAdaptive` strategy - Steady gains with good risk/reward balance
  - **Risk 7-10 (Aggressive)**: `SMC` strategy - Best alpha generator with proven robust scaling
  - **Data-Driven Mapping**: Strategy selection based on comprehensive benchmark analysis across 5 symbols, 9 strategies, and 3 risk levels
  - **Real-Time UI Feedback**: Selected strategy is displayed prominently below the risk profile badge
  - **Persistence**: Strategy selection is saved to `~/.rustrade/settings.json` and restored on startup

### Benchmark Infrastructure
- **Expanded Matrix Benchmarking**: Enhanced benchmark tool to support multi-symbol, multi-strategy analysis
  - Tested 5 major tech stocks (TSLA, NVDA, AAPL, AMD, MSFT)
  - Evaluated 9 distinct strategies across 2 market periods (bearish Dec 2024, bullish Jan 2025)
  - Analyzed risk sensitivity with 3 risk levels (Conservative, Neutral, Aggressive)
- **Key Findings**:
  - SMC strategy showed superior performance at high risk levels (+2.32% in bearish market)
  - MeanReversion optimal at Risk 5, degraded at Risk 8 due to stop-outs
  - TrendRiding identified as dangerous in choppy markets (catastrophic -123% drawdown)

### Code Quality
- Fixed pre-existing test compilation errors (missing `OrderSide` imports)
- All 314 tests passing ✅
- 0 clippy warnings ✅
- Code formatted with `cargo fmt` ✅

## Version 0.74.2 - Fix: Dashboard Settings Visibility (January 2026)

### Bug Fixes
- **Dashboard Metrics**: Fixed issue where the Dashboard displayed default risk scores instead of the persisted ones
  - Updated `DashboardViewModel` to read directly from `SettingsPanel` state
  - Aligned `UserAgent` internal state with persisted settings on startup

## Version 0.74.1 - Fix: Settings Sync on Startup (January 2026)

### Bug Fixes
- **Initialization Sync**: Fixed issue where `Analyst` and `RiskManager` ignored persisted settings on startup
  - Implemented automatic sync in `UserAgent` initialization
  - Settings are now immediately active without requiring a manual "Save" action

## Version 0.74.0 - Settings Persistence (January 2026)

### New Features
- **Settings Persistence**: User configuration is now saved to disk (`~/.rustrade/settings.json`) and automatically loaded on startup
  - **Auto-Save**: Settings are persisted immediately when clicking "Save"
  - **Real-Time Update**: Configuration changes are applied instantly to the running trading engine
  - **Infrastructure**: Robust JSON serialization with fallback to defaults if file is missing

### Security & Maintainability
- **Git Exclusion**: Added `.rustrade/` to `.gitignore` to prevent committing local settings
- **Integration Tests**: Added `settings_persistence_integration.rs` to verify save/load flows
- **Code Quality**: Maintained 0 clippy warnings and passing test suite

## Version 0.73.0 - Settings UI Modernization (January 2026)

### UI/UX Improvements
- **Settings Interface Rework**:
  - Implemented **Master-Detail Layout** with styled sidebar navigation
  - Renamed "System Config" to "Trading Engine" for clarity
  - Added persistent "Save" button in Trading Engine header
  - **Visual Enhancements**:
    - Risk Settings: Color-coded Risk Profile badges (🟢 Conservative / 🟡 Balanced / 🔴 Aggressive)
    - Strategy Settings: Custom-styled input fields with DesignSystem borders
  - **Layout Fixes**:
    - Resolved visibility issues with complex Frame nesting
    - Fixed `Card` component `min_width` constraint causing ScrollArea problems
    - Used `allocate_ui` for explicit height allocation to fill window
    - Applied `auto_shrink([false, false])` to ScrollArea for proper space utilization
  - Settings now properly fill window height with scroll only when content exceeds available space

### Code Quality
- Fixed clippy warnings: removed unused imports in `risk_manager.rs`, added `Default` impl for `Card`
- All 305 tests passing ✅
- 0 clippy warnings ✅

## Version 0.71.0 - Type Safety: Decimal Precision (January 2026)

### Financial Precision Improvements
- **Trailing Stops Decimal Conversion**: Converted `trailing_stops.rs` from `f64` to `Decimal`
  - Eliminated all floating-point precision issues in stop loss calculations
  - Updated `StopState` enum and `TriggerEvent` struct to use `Decimal`
  - Safe conversion using `from_f64_retain()` with fallbacks for ATR values
  - Updated 6 call sites across `analyst.rs`, `signal_processor.rs`, and `position_manager.rs`
  - All 8 trailing stop tests now use `Decimal` literals

### Code Quality
- Removed unused `price_f64` variable in `analyst.rs`
- Maintained 0 clippy warnings
- All 313 tests passing (292 lib + 13 risk + 8 trailing stops)

### Technical Impact
- **Precision**: No more f64 → Decimal → f64 round-trip conversions
- **Safety**: Stop prices maintain full decimal precision
- **Consistency**: All financial calculations now use `Decimal` throughout


## Version 0.70.0 - Code Quality & Maintainability (January 2026)

### Code Refactoring
- **RiskManager Decomposition**: Extracted 963 lines of embedded tests to `tests/risk/risk_manager_tests.rs`
  - Reduced `risk_manager.rs` from 1,740 to 777 lines (-55%)
  - Improved file maintainability and readability
  - All 11 integration tests preserved and passing
- **Production Safety**: Removed 2 `.unwrap()` calls from `chart_panel.rs`
  - Line 19: Safe tab selection with `map_or` fallback
  - Line 108: Safe timestamp conversion with `unwrap_or_else(|| Utc::now())`
  - Eliminates potential panic risks in UI rendering

### Quality Metrics
- ✅ 305 tests passing (292 lib + 13 risk integration tests)
- ✅ 0 clippy warnings (maintained clean state)
- ✅ All code formatted with `cargo fmt`

### Technical Debt Addressed
- **P1 Priority**: God class anti-pattern in RiskManager partially resolved
- **P2 Priority**: Production unwrap() calls eliminated


## Version 0.69.0 (Janvier 2026) - Code Organization Refactoring

- **Binance Infrastructure Decomposition**:
  - **Modular Services**: Split `binance/client.rs` (856 lines) into 4 focused modules:
    - `common.rs` (35 lines) - Shared constants and utilities
    - `market_data.rs` (474 lines) - Market data service
    - `execution.rs` (364 lines) - Order execution service
    - `sector_provider.rs` (39 lines) - Sector classification
  - **Consistent Architecture**: Aligned Binance structure with existing Alpaca pattern for better maintainability
  - **Preserved Functionality**: Zero breaking changes, all tests passing (303 unit tests)

- **UI Settings Modularization**:
  - **Component Extraction**: Decomposed `render_settings_view()` (450 lines) into 4 reusable components:
    - `settings_components/risk_settings.rs` (106 lines) - Simple Mode with risk score slider
    - `settings_components/strategy_settings.rs` (151 lines) - Advanced Mode with all strategy parameters
    - `settings_components/language_settings.rs` (27 lines) - Language selection UI
    - `settings_components/help_about.rs` (26 lines) - Help/Shortcuts/About pages
  - **Reduced Complexity**: Main settings function reduced from 450 to 35 lines (-92%)
  - **Improved Readability**: `ui_components.rs` reduced from 702 to 478 lines (-32%)

- **Code Quality**:
  - Fixed clippy warnings in test suite (same_item_push)
  - All 303 unit tests passing
  - Zero clippy warnings

## Version 0.68.0 (Janvier 2026) - Architectural Refactoring & MVVM


- **System Initialization Refactoring**:
  - **Bootstrap Pattern**: Decomposed the monolithic `system.rs` into focused bootstrap modules (`persistence`, `services`, `agents`) in `src/application/bootstrap/`.
  - **Simplified Startup**: `System::build` now delegates to specialized initializers, improving testability and code organization.

- **Dashboard MVVM**:
  - **ViewModel**: Introduced `DashboardViewModel` to separate UI rendering from data processing logic.
  - **Clean UI Code**: `dashboard.rs` is now strictly focused on layout and rendering, with all P&L calculations and formatting handled by the ViewModel.

- **Domain Purity**:
  - **I18n Migration**: Moved `src/domain/ui` to `src/infrastructure/i18n` to enforce strict Domain-Driven Design (DDD) layers.
  - **Dependency Cleanup**: Removed circular dependencies and infrastructure leakage into the domain.

- **Verification**:
  - Full regression suite passed (294 tests).
  - Validated E2E trading flow with in-memory database.


## Version 0.67.2 (Janvier 2026) - Refactoring: Risk Config & Dashboard

## Version 0.72.0 - Momentum & SMC Strategy Enhancements (January 2026)

### Strategy Improvements
- **Momentum Divergence Refactor**:
  - Implemented actual **Historical RSI Tracking** (last 100 bars) in `AnalysisContext` and `SymbolContext`.
  - Replaced estimation heuristics with precise divergence detection (Higher Low / Lower High).
  - Updated `MomentumStrategy` to utilize this history for significantly improved signal accuracy.
- **Enhanced SMC (Smart Money Concepts)**:
  - **Volume Confirmation**: Order Blocks (OB) now require impulsive volume > `1.5x` average (configurable via `smc_volume_multiplier`).
  - **FVG Mitigation**: Fair Value Gaps are now checked for invalidation/mitigation before signaling, reducing false positives.
  - Updated `StrategyFactory` and `AnalystConfig` to support these new parameters.

### Technical Updates
- **Testing**: Updated all strategy unit tests to support the new `AnalysisContext` structure with historical data.
- **Configuration**: Added `smc_volume_multiplier` to `AnalystConfig`.
- **Code Quality**: Maintained 0 clippy warnings and formatted code base.
