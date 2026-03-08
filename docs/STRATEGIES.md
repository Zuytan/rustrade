# Trading Strategies Documentation

This document provides a comprehensive overview of all modern trading strategies implemented in Rustrade. The legacy SMA-based and basic momentum strategies have been fully deprecated in favor of institutional-grade, statistically sound, and machine learning components.

## Table of Contents

- [Strategy Overview](#strategy-overview)
- [Institutional Strategies](#institutional-strategies)
  - [SMC (Smart Money Concepts)](#smc-smart-money-concepts)
  - [Order Flow](#order-flow)
- [Statistical & Quantitative](#statistical--quantitative)
  - [Z-Score Mean Reversion](#z-score-mean-reversion)
  - [Statistical Momentum](#statistical-momentum)
- [Adaptive & Meta](#adaptive--meta)
  - [Regime Adaptive](#regime-adaptive)
  - [Ensemble](#ensemble)
- [Machine Learning](#machine-learning)
  - [ML inference](#ml-inference)
- [Configuration](#configuration)

---

## Strategy Overview

| Strategy | Type | Market Condition | Risk Level | Key Indicators |
|----------|------|------------------|------------|----------------|
| SMC | Institutional | Trending / Reversal | Medium | Order Blocks, FVG, MSS |
| Order Flow | Institutional | High Liquidity | Medium | OFI, Cumulative Delta, VP |
| ZScoreMR | Quantitative | Ranging | Low | Z-Score of Returns, Volatility |
| StatMomentum | Quantitative | Trending | High | Moving Linear Reg, R² |
| RegimeAdaptive | Adaptive | All | Variable | ADX, Trend Detection, Variance |
| Ensemble | Meta | All | Low | Multi-Strategy Vote |
| ML | Predictive | All | Medium | ONNX / SmartCore Models |

---

## Institutional Strategies

### SMC (Smart Money Concepts)
Focuses on identifying institutional order flow patterns, tracing footprints left by large players.

**Core Concepts:**
- **Order Blocks (OB):** The last opposing candle before an impulsive move. Marks accumulation/distribution zones.
- **Fair Value Gaps (FVG):** Price imbalances formed by impulsive 3-candle sequences without retracement footprint.
- **Market Structure Shift (MSS):** Confirmation of trend change via breaks of recent swings.

**Algorithm:**
Generates Buy/Sell proposals when price retraces into a confirmed FVG or OB zone with subsequent directional confirmation.

### Order Flow
Analyzes the microstructure of the market by keeping track of the order book dynamics and volume distribution.

**Core Concepts:**
- **Order Flow Imbalance (OFI):** Measures the net pressure between bid and ask volumes.
- **Cumulative Delta:** Tracks the running total of aggressive market buying vs selling over the session.
- **Volume Profile:** Identifies high and low volume nodes (HVN/LVN) serving as support/resistance.

---

## Statistical & Quantitative

### Z-Score Mean Reversion (ZScoreMR)
A highly mathematical mean-reversion algorithm based on the statistical Z-Score formulation.

**Algorithm:**
Rather than relying on RSI or Bollinger Bands, it calculates the Z-Score of log returns over a lookback window. When the Z-Score exceeds extreme statistical bounds (e.g., ±2.0 σ), the strategy assumes price has decoupled from fair value and takes a contrarian position, aiming to exit as the Z-Score returns to 0.

### Statistical Momentum (StatMomentum)
A mathematically rigorous approach to trend following.

**Algorithm:**
Uses Moving Linear Regression applied to price data. It calculates the slope and the corresponding R² value. If the slope is steep and the R² indicates a very strong linear fit (low variance noise), it generates a momentum entry. Provides mathematically quantified trend conviction compared to lagging SMAs.

---

## Adaptive & Meta

### Regime Adaptive
Automatically detects the current market environment and delegates signal generation to the most appropriate sub-strategy.

**Regimes Supported:**
1. **Trending Up / Trending Down:** Triggered by high ADX. Uses trend strategies (e.g., SMC or StatMomentum).
2. **Choppy/Ranging:** Triggered by low ADX. Uses mean-reversion (ZScoreMR).

### Ensemble
A meta-strategy that aggregates signals from multiple robust child strategies using a weighted voting system.
Provides highest confidence and lowest drawdown by ensuring consensus among uncorrelated models.

---

## Machine Learning

### ML Inference
Uses pre-trained models to predict price movements based on a rich state representation of the market.

**Features included:**
ONNX Runtime support, SmartCore legacy support. Ingests dozens of features including normalized momentum, realized volatility, OFI, and Hurst exponent.

---

## Configuration

### Environment Variables
```bash
# Strategy Selection
STRATEGY_MODE=smc  # smc, zscoremr, statmomentum, orderflow, regimeadaptive, ensemble, ml
```

### Programmatic Setup
The `StrategyFactory` instantiates strategies directly from the `StrategyMode` enum seamlessly.
