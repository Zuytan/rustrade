# Statistical Features Documentation

This document describes the advanced statistical features implemented in the Rustrade feature engineering pipeline. These features are designed to capture market regime, volatility, and trend dynamics for use in quantitative strategies and machine learning models.

## 1. Hurst Exponent (`hurst_exponent`)

### Description
The Hurst Exponent is a measure of the long-term memory of a time series. It relates to the autocorrelations of the time series and the rate at which these decrease as the lag between pairs of values increases.

### Interpretation
- **H < 0.5**: Mean-reverting series. The closer to 0, the stronger the mean reversion.
- **H = 0.5**: Random walk (geometric Brownian motion). No predictable pattern.
- **H > 0.5**: Trending series. The closer to 1, the stronger the trend persistence.

### Usage
- Used to distinguish between trending and ranging market regimes.
- Strategies can switch between trend-following (H > 0.6) and mean-reversion (H < 0.4) logic.

## 2. Skewness (`skewness`)

### Description
Skewness measures the asymmetry of the probability distribution of returns about its mean.

### Interpretation
- **Negative Skew**: The left tail is longer or fatter. Indicates a higher probability of large negative returns (crashes).
- **Positive Skew**: The right tail is longer. Indicates a higher probability of large positive returns.
- **Zero Skew**: Symmetric distribution.

### Usage
- Risk management: Avoid long positions in assets with highly negative skewness during volatile periods.
- Feature for ML models to predict tail risks.

## 3. ATR-Normalized Momentum (`momentum_normalized`)

### Description
Standard momentum measures the rate of change of price. However, raw price changes are hard to compare across assets or volatility regimes. This feature normalizes the momentum by the Average True Range (ATR).

### Calculation
`Momentum_Normalized = (Close - Close[Lookback]) / ATR[Lookback]`

### Interpretation
- **Positive**: Upward trend stronger than recent volatility.
- **Negative**: Downward trend stronger than recent volatility.
- **Magnitude**: Indicates the strength of the trend relative to noise. A value > 1 means the price moved more than one average daily range in the direction of the trend.

### Usage
- Core signal for `StatisticalMomentum` strategy.
- Filters out "choppy" trends where price movement is significant but within the bounds of normal volatility.

## 4. Realized Volatility (`realized_volatility`)

### Description
Realized Volatility measures the actual historical variation of returns over a specific lookback period. Unlike implied volatility (from options), this is calculated from underlying price action.

### Calculation
Standard deviation of log returns over the lookback window, typically annualized.

### Interpretation
- **High Value**: High uncertainty and risk. fast price movements.
- **Low Value**: Stable market conditions.

### Usage
- **Volatility Targeting**: Adjust position sizes inversely to realized volatility to target a constant risk level (e.g., target 10% annual vol).
- **Regime Detection**: Switch to conservative strategies when realized volatility spikes.
