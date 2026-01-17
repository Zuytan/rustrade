# Feature Specifications & Dependencies

This document tracks features and their cross-cutting concerns.
**Use this for Impact Analysis.**

## Trading Engine

### Strategy Execution
- **Depends on**: `Sentinel` (Data), `RiskManager` (Validation)
- **Impacts**: `Executor` (Orders), `UI` (Logs/Charts)
- **Files**: `src/application/strategies/*`, `src/application/agents/analyst.rs`

### Risk Management
- **Depends on**: `AccountState` (Broker)
- **Impacts**: `OrderRejection`, `PositionSizing`
- **Files**: `src/domain/risk/*`, `src/application/risk_management/*`

### Position Management
- **Depends on**: `Executor` (Fills)
- **Impacts**: `RiskCheck` (Buying Power), `UI` (Portfolio)
- **Files**: `src/application/risk_management/position_manager.rs`

## User Interface (UI)

### Dashboard
- **Depends on**: `SystemClient` (Data feed), `I18n`
- **Impacts**: User perception of system state
- **Files**: `src/interfaces/dashboard.rs`

### Settings
- **Depends on**: `SettingsPersistence` (SQLite)
- **Impacts**: `RiskConfig`, `StrategyConfig` (Hot reload)
- **Files**: `src/interfaces/ui_components.rs`, `src/infrastructure/settings_persistence.rs`

## Cross-Cutting Concerns

| Feature | Impacted Modules | Constraint |
|---------|------------------|------------|
| **Internationalization (i18n)** | UI, Logs | All user strings must be keys in `locales/` |
| **Decimal Precision** | Domain, App, Infra, UI | Never cast to f64 for display intermediate calc |
| **Dark Mode** | UI Components | Use `DesignSystem` constants |

## Change Propagation Checklist

If you modify **X**, check **Y**:

- **Strategy Logic** → Check `Backtest`, `Analyst`, `UI Settings`
- **Risk Rule** → Check `ValidationContext`, `UI Risk Tab`, `Tests`
- **Data Model** → Check `Database`, `Serialization`, `UI Display`
