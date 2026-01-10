# Trading Strategies Documentation

This document provides a comprehensive overview of all trading strategies implemented in Rustrade.

## Table of Contents

- [Strategy Overview](#strategy-overview)
- [Trend Following Strategies](#trend-following-strategies)
  - [Dual SMA (Standard)](#dual-sma-standard)
  - [Advanced Triple Filter](#advanced-triple-filter)
  - [Trend Riding](#trend-riding)
- [Mean Reversion Strategies](#mean-reversion-strategies)
  - [Mean Reversion (Bollinger)](#mean-reversion-bollinger)
  - [VWAP](#vwap)
- [Momentum Strategies](#momentum-strategies)
  - [Breakout](#breakout)
  - [Momentum Divergence](#momentum-divergence)
- [Institutional Strategies](#institutional-strategies)
  - [SMC (Smart Money Concepts)](#smc-smart-money-concepts)
- [Adaptive Strategies](#adaptive-strategies)
  - [Dynamic (Regime-Based)](#dynamic-regime-based)
  - [Ensemble](#ensemble)
- [Configuration](#configuration)

---

## Strategy Overview

| Strategy | Type | Market Condition | Risk Level | Key Indicators |
|----------|------|------------------|------------|----------------|
| DualSMA | Trend | Trending | Medium | SMA Crossover |
| Advanced | Trend | Strong Trends | Low | SMA + RSI + MACD + ADX |
| TrendRiding | Trend | Long Trends | Medium | EMA + Trailing Stop |
| MeanReversion | Contrarian | Ranging | Medium | Bollinger Bands + RSI |
| VWAP | Contrarian | Ranging | Low | Volume-Weighted Price |
| Breakout | Momentum | Volatility | High | Volume + Range |
| Momentum | Momentum | Divergences | Medium | RSI Divergence |
| SMC | Institutional | All | Medium | Order Blocks + FVG |
| Dynamic | Adaptive | All | Variable | Regime Detection |
| Ensemble | Meta | All | Low | Multi-Strategy Vote |

---

## Trend Following Strategies

### Dual SMA (Standard)

The simplest and most reliable trend-following strategy based on moving average crossovers.

#### Algorithm

```
BUY Signal (Golden Cross):
  Fast SMA crosses ABOVE Slow SMA
  
SELL Signal (Death Cross):
  Fast SMA crosses BELOW Slow SMA
```

#### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `fast_period` | 2 | Fast SMA lookback period |
| `slow_period` | 5 | Slow SMA lookback period |

#### When to Use

- Trending markets with clear directional moves
- Medium to high volatility environments
- When you want fewer false signals than single-indicator strategies

---

### Advanced Triple Filter

A multi-layered confirmation system that requires agreement from multiple indicators before generating signals.

#### Algorithm

```
BUY Signal (all must pass):
  1. Golden Cross (Fast SMA > Slow SMA)
  2. Price > Trend SMA (above long-term trend)
  3. RSI < Overbought threshold (not overextended)
  4. MACD Histogram > 0 and rising (positive momentum)
  5. ADX > Threshold (strong trend)

SELL Signal (all must pass):
  1. Death Cross (Fast SMA < Slow SMA)
  2. Price < Trend SMA (below long-term trend)
  3. RSI > Oversold threshold
  4. MACD Histogram < 0
```

#### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `trend_period` | 20 | Long-term trend SMA period |
| `rsi_period` | 14 | RSI calculation period |
| `rsi_overbought` | 75 | RSI overbought threshold |
| `rsi_oversold` | 25 | RSI oversold threshold |
| `adx_period` | 14 | ADX smoothing period |
| `adx_threshold` | 25 | Minimum trend strength |

#### When to Use

- When you want high-conviction signals with fewer trades
- Strong trending markets
- When capital preservation is priority

---

### Trend Riding

Designed to capture extended trends by staying in positions longer using trailing mechanisms.

#### Algorithm

```
ENTRY:
  Golden Cross AND Price > Long-term EMA

EXIT:
  Price drops below trailing stop (EMA - buffer)
  OR Death Cross occurs
```

#### Key Features

- Uses EMA (Exponential Moving Average) for faster reaction
- Trailing stop follows price higher, never lower
- Buffer zone prevents premature exits on noise

#### When to Use

- Long-running trends (hours to days)
- When you want to "let winners run"
- Lower-frequency trading

---

## Mean Reversion Strategies

### Mean Reversion (Bollinger)

Capitalizes on price returning to the mean after extreme moves, using Bollinger Bands.

#### Algorithm

```
BUY Signal:
  Price touches/crosses BELOW Lower Bollinger Band
  AND RSI indicates oversold (< 30)

SELL Signal:
  Price touches/crosses ABOVE Upper Bollinger Band
  AND RSI indicates overbought (> 70)
```

#### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `bb_period` | 20 | Bollinger Band period |
| `bb_std_dev` | 2.0 | Standard deviation multiplier |
| `rsi_period` | 14 | RSI period |

#### When to Use

- Range-bound, sideways markets
- Low ADX environments (weak trends)
- Counter-trend opportunities

---

### VWAP

Volume-Weighted Average Price strategy for intraday mean reversion.

#### Algorithm

```
VWAP = Σ(Price × Volume) / Σ(Volume)

BUY Signal:
  Price significantly BELOW VWAP (oversold relative to volume)
  AND showing reversal candle pattern

SELL Signal:
  Price significantly ABOVE VWAP (overbought relative to volume)
  AND showing reversal pattern
```

#### Key Features

- Incorporates volume into price analysis
- Institutional benchmark price
- Resets daily (intraday strategy)

#### When to Use

- Intraday trading
- High-volume, liquid assets
- When volume confirmation is important

---

## Momentum Strategies

### Breakout

Captures explosive moves when price breaks out of consolidation zones.

#### Algorithm

```
BUY Signal:
  Price breaks ABOVE recent high (N-period high)
  AND Volume surge (> 1.5x average volume)
  AND Range expansion (current range > average range)

SELL Signal:
  Price breaks BELOW recent low
  OR Trailing stop hit
```

#### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `lookback` | 20 | Period for high/low detection |
| `volume_multiplier` | 1.5 | Volume surge threshold |

#### When to Use

- After consolidation periods
- When volatility is expanding
- News-driven moves

---

### Momentum Divergence

Detects when price and momentum indicators diverge, signaling potential reversals.

#### Algorithm

```
Bullish Divergence (BUY):
  Price makes LOWER low
  BUT RSI makes HIGHER low
  → Momentum weakening in downtrend

Bearish Divergence (SELL):
  Price makes HIGHER high
  BUT RSI makes LOWER high
  → Momentum weakening in uptrend
```

#### Key Features

- Identifies exhaustion in trends
- Requires pattern recognition over multiple swings
- Higher accuracy with confirmation

#### When to Use

- End of extended trends
- When looking for reversal entries
- Combined with support/resistance levels

---

## Institutional Strategies

### SMC (Smart Money Concepts)

Focuses on identifying institutional order flow patterns.

#### Core Concepts

**Order Blocks (OB)**
```
Bullish OB: Last bearish candle before strong bullish move
Bearish OB: Last bullish candle before strong bearish move
→ Mark zones where institutions accumulated positions
```

**Fair Value Gaps (FVG)**
```
Bullish FVG: Gap between Candle 1 High and Candle 3 Low
  (Candle 2 is impulsive bullish)
Bearish FVG: Gap between Candle 1 Low and Candle 3 High
  (Candle 2 is impulsive bearish)
→ Imbalances that price tends to fill
```

**Market Structure Shift (MSS)**
```
Bullish MSS: Price closes above recent swing high
Bearish MSS: Price closes below recent swing low
→ Confirms change in market direction
```

#### Algorithm

```
BUY Signal:
  1. Bullish FVG detected
  2. Price near bullish Order Block
  3. Bullish MSS confirmed

SELL Signal:
  1. Bearish FVG detected
  2. Price near bearish Order Block
  3. Bearish MSS confirmed
```

#### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `ob_lookback` | 20 | Order Block detection period |
| `min_fvg_size_pct` | 0.1 | Minimum FVG size (0.1%) |

#### When to Use

- Any market condition
- When trading alongside institutions
- Higher timeframes preferred

---

## Adaptive Strategies

### Dynamic (Regime-Based)

Automatically switches between strategies based on detected market regime.

#### Regime Detection

```
Market Regimes:
  1. Trending Up: ADX > threshold, positive slope
  2. Trending Down: ADX > threshold, negative slope  
  3. Ranging: ADX < threshold, low variance
  4. Volatile: High variance, unstable direction
```

#### Strategy Mapping

| Regime | Active Strategy |
|--------|-----------------|
| Trending (Up/Down) | TrendRiding or Advanced |
| Ranging | MeanReversion or VWAP |
| Volatile | Reduced sizing, Breakout |

#### Key Features

- Uses ADX for trend strength
- Linear regression for trend direction
- Variance analysis for volatility

---

### Ensemble

Meta-strategy that combines multiple strategies through a voting mechanism.

#### Algorithm

```
For each candle:
  1. Run all component strategies
  2. Collect BUY/SELL/HOLD votes
  3. Weight votes by strategy confidence
  4. Generate signal if consensus reached

Signal = Weighted vote if > threshold (e.g., 60%)
```

#### Component Strategies

1. DualSMA
2. Advanced Triple Filter
3. Mean Reversion
4. VWAP

#### When to Use

- When seeking high-conviction signals
- Risk-averse trading
- When no single strategy dominates

---

## Configuration

### Environment Variables

```bash
# Strategy Selection
STRATEGY_MODE=advanced  # standard, advanced, dynamic, trendriding, meanreversion, vwap, breakout, momentum, smc, ensemble

# Common Parameters
ADX_PERIOD=14
ADX_THRESHOLD=25.0
RSI_THRESHOLD=75.0

# VWAP Specific
VWAP_LOOKBACK=20

# Breakout Specific
BREAKOUT_VOLUME_MULT=1.5

# SMC Specific
SMC_OB_LOOKBACK=20
SMC_MIN_FVG_PCT=0.1
```

### Programmatic Configuration

```rust
use rustrade::application::strategies::StrategyFactory;
use rustrade::domain::market::StrategyMode;

// Create strategy instance
let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);

// Use in analysis
let signal = strategy.analyze(&context);
```

---

## Further Reading

- [Global App Description](../GLOBAL_APP_DESCRIPTION.md) - Full system architecture
- [Risk Management](../GLOBAL_APP_DESCRIPTION.md#4-risk-management-system) - Position sizing and risk controls
