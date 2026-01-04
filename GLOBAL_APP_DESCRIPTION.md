# Rustrade Application Description

## Overview
Rustrade is a high-performance, algorithmic trading bot written in Rust, designed for reliability, concurrency, and modularity. It supports multiple asset classes (Stocks, Crypto) and brokers (Alpaca, OANDA, Mock).

## Core Features
- **Multi-Strategy Engine**: Supports Standard (Dual SMA), Advanced (Triple Filter: SMA+RSI+MACD+ADX), Dynamic Regime Adaptive, and Mean Reversion strategies.
- **Market Regime Detection**: Automatically detects Bull, Bear, Sideways, and Volatile regimes.
- **Risk Management**:
  - Position sizing based on account risk (e.g., 1% per trade).
  - Global circuit breakers (Day Loss Limit, Drawdown Limit).
  - Correlation filters to prevent over-exposure.
  - Sector exposure limits.
- **Data Pipeline**:
  - Real-time market data streaming (Polygon/Alpaca/Mock).
  - Historical data warmup for indicators.
  - Dynamic symbol scanning (Top Movers).
- **Execution**:
  - Order throttling.
  - Slippage and commission modeling.
  - Portfolio state management.

## Latest Updates (Version 0.40.1)
- **RiskAppetite Propagation Fix**: Fixed `DynamicRegimeStrategy` to properly receive all risk_appetite parameters:
  - Added `DynamicRegimeConfig` struct for full parameter support.
  - `StrategyFactory` and `system.rs` now pass `macd_requires_rising`, `trend_tolerance_pct`, `macd_min_threshold`, `adx_threshold` to Dynamic strategy.
  - Previously hardcoded conservative defaults now respect user's configured risk profile.

## Version 0.40.0
- **ADX Integration**: implemented Average Directional Index (ADX) to filter out weak trends in `AdvancedTripleFilterStrategy`.
  - Configurable `ADX_PERIOD` and `ADX_THRESHOLD`.
  - Manual ADX implementation for precision.
- **Refactoring**: Updated `FeatureEngineeringService` to use `Candle` data (High/Low/Close) for advanced indicators.

## Architecture
- **Agents**: Sentinel (Data ingestion), Scanner (Opportunity finding), Analyst (Strategy execution), RiskManager (Safety), Executor (Order placement).
- **Domain-Driven Design (DDD)**: Clear separation of Domain, Application, and Infrastructure layers.
- **Async/Await**: Built on Tokio for non-blocking concurrency.
