# Rustrade - Historique des Versions

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
