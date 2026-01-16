# Rustrade - Historique des Versions

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
