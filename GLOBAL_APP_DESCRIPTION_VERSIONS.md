# Rustrade - Historique des Versions

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
