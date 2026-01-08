# Rustrade Application Description

## Overview
Rustrade is a high-performance, algorithmic trading bot written in Rust, designed for reliability, concurrency, and modularity. It supports multiple asset classes (Stocks, Crypto) and brokers (Alpaca, OANDA, Binance, Mock).

## Core Features
- **Multi-Strategy Engine**: Supports Standard (Dual SMA), Advanced (Triple Filter: SMA+RSI+MACD+ADX), Dynamic Regime Adaptive, and Mean Reversion strategies.
- **Market Regime Detection**: Automatically detects Bull, Bear, Sideways, and Volatile regimes.
- **Risk Management**:
  - **Modular Architecture**: Chain of Responsibility pattern with independent validators.
  - **Persistent State**: Dedicated State Manager ensures critical metrics (HWM, Daily Loss) survive restarts.
  - Position sizing based on account risk (e.g., 1% per trade).
  - Global circuit breakers (Day Loss Limit, Drawdown Limit) and infrastructure-level circuit breakers for API resilience.
  - Dynamic volatility-based position sizing (ATR-based multiplier).
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

## Latest Updates (Version 0.52.0)
- **Dynamic Risk Management (P2)**:
  - **Volatility Adaptation**: Implemented `VolatilityManager` to calculate ATR-based multipliers, dynamically scaling position sizes based on market conditions.
  - **Infrastructure Resilience**: Integrated `CircuitBreaker` pattern into `Alpaca` and `Binance` services to prevent system stalls during API outages.
- **Centralized Cost Model (P1)**:
  - Unified transaction cost logic (commission/slippage) via `FeeModel` trait, ensuring consistency between simulation and live trading.
- **API Resilience (P0)**:
  - Implemented `HttpClientFactory` with standard `ExponentialBackoff` retry policies for all HTTP integrations.

## Version 0.50.0
- **P0 Critical Security Fixes**:
  - **Risk State Persistence**: Implemented `SqliteRiskStateRepository` to persist critical risk metrics (Daily Loss, HWM) across restarts, preventing verification bypass ("Amnesia").
  - **Blind Liquidation (Panic Mode)**: `RiskManager` now bypasses price checks during emergency liquidations, ensuring market exit even without data feed.
- **P2 & P3 Metrics & Hardening**:
  - **Performance Metrics**: Added Rolling Sharpe Ratio (30d), Win Rate (30d), and FIFO PnL reconstruction.
  - **Logic Hardening**: Eliminated unsafe unwraps in Analyst components.

## Version 0.46.0
- **Unified Configuration Modes**: Two distinct modes for system configuration:
  - **Simple (Novice)**: Single "Risk Appetite Score" (1-10) slider that auto-tunes all 12+ deeply technical parameters. Includes "Conservative", "Balanced", and "Aggressive" profile visualizers.
  - **Advanced (Expert)**: Full granular control over every parameter (SMA periods, RSI thresholds, etc.).
- **Dynamic Configuration System**: Runtime adjustment of Risk and Strategy parameters without restarting.
- **System Config UI**: New settings tab with granular controls (Risk Limits, SMA Periods, RSI Thresholds, etc.).
- **Internationalization (I18n)**: Full localization of the new configuration UI including helpful tooltips for every parameter.
- **Market Sentiment Analysis**: Integrated "Fear & Greed Index" (Crypto) analysis. Risk Manager automatically reduces position sizing by 50% during "Extreme Fear" periods. "Market Mood" is visualized on the Dashboard with dynamic color coding.

## Version 0.43.0
- **Dynamic Dashboard Metrics**: User Interface now reflects real-time trading statistics:
  - **Dynamic Win Rate**: Replaced static chart with dynamic arc visualization based on portfolio history.
  - **Monte Carlo Integration**: Simulation uses actual Average Win/Loss percentages derived from closed trades.
  - **Risk Score Display**: Dynamic "Low/Medium/High" label and color coding based on risk appetite configuration.

## Version 0.42.0
- **Multi-Timeframe Analysis Infrastructure**: Added comprehensive multi-timeframe support:
  - New domain types: `Timeframe` enum (1Min, 5Min, 15Min, 1Hour, 4Hour, 1Day) with API conversions for Alpaca, Binance, and OANDA.
  - `TimeframeCandle` struct for aggregated OHLCV data across timeframes.
  - `TimeframeAggregator` service for real-time candle aggregation (1-min → higher timeframes).
  - Extended `SymbolContext` to track multiple timeframes simultaneously.
  - Configuration support via `PRIMARY_TIMEFRAME`, `TIMEFRAMES`, and `TREND_TIMEFRAME` environment variables.
  - **Performance Improvement**: Reduced `TREND_SMA_PERIOD` from 2000 to 50 (93% reduction in warmup candles: ~2200 → ~55).
  - Preset configurations for Day Trading, Swing Trading, Crypto 24/7, and Scalping strategies.
  - 14 new unit tests for timeframe logic and aggregation (171 total tests passing).
- **Multi-Timeframe Strategy Integration (Phase 3)**:
  - Extended `AnalysisContext` with `TimeframeFeatures` and multi-timeframe helper methods.
  - `AdvancedTripleFilterStrategy` now validates higher timeframe trend confirmation before buy signals.
  - `DynamicRegimeStrategy` uses highest timeframe ADX for more reliable regime detection.
  - Helper methods: `higher_timeframe_confirms_trend()`, `multi_timeframe_trend_strength()`, `all_timeframes_bullish()`, `get_highest_timeframe_adx()`.
  - Backward compatible: existing strategies work without multi-timeframe data (optional feature).
  - Improved signal quality: blocks trades when primary timeframe signal conflicts with higher timeframe trend.

## Version 0.41.0
- **Binance Integration**: Added Binance as a third broker option for cryptocurrency trading:
  - Implemented `BinanceMarketDataService` with REST API and WebSocket support.
  - Implemented `BinanceExecutionService` with HMAC-SHA256 authentication.
  - Added `BinanceSectorProvider` for crypto categorization (Layer1, DeFi, Layer2, etc.).
  - Symbol normalization support (BTCUSDT ↔ BTC/USDT).
  - Top movers scanner using 24h ticker API.
  - Candle caching integration for historical data.
  - Configuration via `MODE=binance` environment variable.

## Version 0.40.1
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
