use crate::domain::trading::types::Candle;
use anyhow::Result;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents the current market regime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketRegimeType {
    TrendingUp,
    TrendingDown,
    Ranging,
    Volatile,
    Unknown,
}

impl fmt::Display for MarketRegimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarketRegimeType::TrendingUp => write!(f, "Trending Up"),
            MarketRegimeType::TrendingDown => write!(f, "Trending Down"),
            MarketRegimeType::Ranging => write!(f, "Ranging"),
            MarketRegimeType::Volatile => write!(f, "Volatile"),
            MarketRegimeType::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Detailed market regime information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketRegime {
    pub regime_type: MarketRegimeType,
    pub confidence: Decimal, // 0.0 to 1.0
    pub volatility_score: Decimal,
    pub trend_strength: Decimal,
}

impl MarketRegime {
    pub fn new(
        regime_type: MarketRegimeType,
        confidence: Decimal,
        volatility_score: Decimal,
        trend_strength: Decimal,
    ) -> Self {
        use rust_decimal_macros::dec;
        Self {
            regime_type,
            confidence: confidence.clamp(dec!(0.0), dec!(1.0)),
            volatility_score,
            trend_strength,
        }
    }

    pub fn unknown() -> Self {
        Self {
            regime_type: MarketRegimeType::Unknown,
            confidence: Decimal::ZERO,
            volatility_score: Decimal::ZERO,
            trend_strength: Decimal::ZERO,
        }
    }
}

/// Service for detecting market regime from price action
pub struct MarketRegimeDetector {
    window_size: usize,
    adx_threshold: Decimal,
    volatility_threshold: Decimal,
}

impl MarketRegimeDetector {
    pub fn new(window_size: usize, adx_threshold: Decimal, volatility_threshold: Decimal) -> Self {
        Self {
            window_size,
            adx_threshold,
            volatility_threshold,
        }
    }

    pub fn detect_from_features(
        &self,
        hurst: Option<Decimal>,
        volatility: Option<Decimal>,
        _skewness: Option<Decimal>,
    ) -> Result<MarketRegime> {
        // Enhanced Detection using Statistical Features
        use rust_decimal_macros::dec;

        // 1. Volatility Check
        let is_volatile = volatility
            .map(|v| v > self.volatility_threshold)
            .unwrap_or(false);
        if is_volatile {
            return Ok(MarketRegime::new(
                MarketRegimeType::Volatile,
                dec!(0.8), // High confidence if directly measured
                volatility.unwrap_or(Decimal::ZERO) * dec!(100.0),
                Decimal::ZERO,
            ));
        }

        // 2. Trend vs Mean Reversion using Hurst
        if let Some(h) = hurst {
            if h > dec!(0.6) {
                // Strong Trending Behavior
                return Ok(MarketRegime::new(
                    MarketRegimeType::TrendingUp, // Direction needs price action, defaulting to Generic Trend or need direction input
                    // Hurst > 0.6 indicates trending behavior, but direction is ambiguous from Hurst alone.
                    // Direction determination requires additional context (e.g., slope or momentum).
                    // Current implementation maps to TrendingUp as a generic placeholder for "Trending".
                    (h - dec!(0.5)) * dec!(2.0), // Confidence scales with Hurst
                    Decimal::ZERO,
                    h * dec!(100.0),
                ));
            } else if h < dec!(0.4) {
                // Mean Reverting
                return Ok(MarketRegime::new(
                    MarketRegimeType::Ranging,
                    (dec!(0.5) - h) * dec!(2.0),
                    Decimal::ZERO,
                    Decimal::ZERO,
                ));
            }
        }

        // Fallback to Unknown if no features
        Ok(MarketRegime::unknown())
    }

    pub fn detect(&self, candles: &[Candle]) -> Result<MarketRegime> {
        if candles.len() < self.window_size {
            return Ok(MarketRegime::unknown());
        }

        let recent_candles = &candles[candles.len().saturating_sub(self.window_size)..];

        // 1. Calculate Volatility (ATR / Price)
        let atr = self.calculate_atr(recent_candles, 14);
        let current_price = recent_candles
            .last()
            .expect("recent_candles slice guaranteed non-empty by window_size check")
            .close;
        let volatility_score = if current_price > Decimal::ZERO {
            use rust_decimal_macros::dec;
            (atr / current_price) * dec!(100.0)
        } else {
            Decimal::ZERO
        };

        // 2. Calculate Trend Strength (ADX equivalent approximation)
        let trend_strength = self.calculate_trend_strength(recent_candles);
        let is_uptrend = self.is_uptrend(recent_candles);

        // 3. Determine Regime
        let regime_type = if trend_strength > self.adx_threshold {
            if is_uptrend {
                MarketRegimeType::TrendingUp
            } else {
                MarketRegimeType::TrendingDown
            }
        } else if volatility_score > self.volatility_threshold {
            MarketRegimeType::Volatile
        } else {
            MarketRegimeType::Ranging
        };

        // 4. Calculate Confidence (simplified)
        use rust_decimal_macros::dec;
        let confidence = match regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                let strength_excess = if trend_strength > self.adx_threshold {
                    trend_strength - self.adx_threshold
                } else {
                    Decimal::ZERO
                };
                (dec!(0.5) + strength_excess * dec!(0.02)).min(Decimal::ONE)
            }
            MarketRegimeType::Volatile => {
                let vol_excess = if volatility_score > self.volatility_threshold {
                    volatility_score - self.volatility_threshold
                } else {
                    Decimal::ZERO
                };
                (dec!(0.5) + vol_excess * dec!(0.1)).min(Decimal::ONE)
            }
            MarketRegimeType::Ranging => dec!(0.6), // Default confidence for ranging
            MarketRegimeType::Unknown => Decimal::ZERO,
        };

        Ok(MarketRegime::new(
            regime_type,
            confidence,
            volatility_score,
            trend_strength,
        ))
    }

    fn calculate_atr(&self, candles: &[Candle], period: usize) -> Decimal {
        if candles.len() < period + 1 {
            return Decimal::ZERO;
        }

        let mut tr_sum = Decimal::ZERO;
        for i in 1..candles.len() {
            let high = candles[i].high;
            let low = candles[i].low;
            let close_prev = candles[i - 1].close;

            let tr = (high - low)
                .max((high - close_prev).abs())
                .max((low - close_prev).abs());

            // Simple average for this window (could be smoothed)
            if i >= candles.len() - period {
                tr_sum += tr;
            }
        }

        tr_sum / Decimal::from(period)
    }

    fn calculate_trend_strength(&self, candles: &[Candle]) -> Decimal {
        let n = candles.len();
        if n < 2 {
            return Decimal::ZERO;
        }

        let prices: Vec<Decimal> = candles.iter().map(|c| c.close).collect();

        // Linear regression: y = mx + c
        let n_dec = Decimal::from(n);
        let x_sum: Decimal = (0..n).map(Decimal::from).sum();
        let y_sum: Decimal = prices.iter().sum();
        let xy_sum: Decimal = prices
            .iter()
            .enumerate()
            .map(|(i, &p)| Decimal::from(i) * p)
            .sum();
        let x2_sum: Decimal = (0..n).map(|i| Decimal::from(i * i)).sum();

        let denominator = n_dec * x2_sum - x_sum * x_sum;
        if denominator == Decimal::ZERO {
            return Decimal::ZERO;
        }

        let slope = (n_dec * xy_sum - x_sum * y_sum) / denominator;
        use rust_decimal_macros::dec;
        let first_price = prices[0].max(dec!(0.0001));

        (slope / first_price).abs() * dec!(1000.0)
    }

    fn is_uptrend(&self, candles: &[Candle]) -> bool {
        if candles.len() < 2 {
            return false;
        }
        let first = candles.first().expect("candles verified len >= 2").close;
        let last = candles.last().expect("candles verified len >= 2").close;
        last > first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::Candle;
    use chrono::Utc;
    use rust_decimal::Decimal;

    fn create_candle(price: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            timestamp: Utc::now().timestamp(),
            open: Decimal::from_f64_retain(price).unwrap(),
            high: Decimal::from_f64_retain(price + 1.0).unwrap(),
            low: Decimal::from_f64_retain(price - 1.0).unwrap(),
            close: Decimal::from_f64_retain(price).unwrap(),
            volume: Decimal::from_f64_retain(1000.0).unwrap(),
        }
    }

    #[test]
    fn test_regime_detection_uptrend() {
        use rust_decimal_macros::dec;
        let detector = MarketRegimeDetector::new(10, dec!(25.0), dec!(2.0));
        let mut candles = Vec::new();
        // Generate strong uptrend
        for i in 0..20 {
            candles.push(create_candle(100.0 + (i as f64) * 2.0));
        }

        let regime = detector.detect(&candles).unwrap();
        assert!(matches!(
            regime.regime_type,
            MarketRegimeType::TrendingUp | MarketRegimeType::Ranging
        ));
    }

    #[test]
    fn test_detect_from_features_hurst() {
        use rust_decimal_macros::dec;
        let detector = MarketRegimeDetector::new(10, dec!(25.0), dec!(2.0));
        // Hurst > 0.6 -> Trending
        let regime = detector
            .detect_from_features(Some(dec!(0.7)), Some(Decimal::ZERO), None)
            .unwrap();
        // Without direction, we define it maps to TrendingUp as placeholder for "Trend"
        assert_eq!(regime.regime_type, MarketRegimeType::TrendingUp);
        assert!(regime.confidence > dec!(0.0));

        // Hurst < 0.4 -> Mean Reversion (Ranging)
        let regime_mr = detector
            .detect_from_features(Some(dec!(0.3)), Some(dec!(0.0)), None)
            .unwrap();
        assert_eq!(regime_mr.regime_type, MarketRegimeType::Ranging);
    }

    #[test]
    fn test_detect_from_features_volatility() {
        use rust_decimal_macros::dec;
        let detector = MarketRegimeDetector::new(10, dec!(25.0), dec!(2.0)); // Vol thresh 2.0
        // Volatility 3.0 > 2.0 -> Volatile
        let regime = detector
            .detect_from_features(None, Some(dec!(3.0)), None)
            .unwrap();
        assert_eq!(regime.regime_type, MarketRegimeType::Volatile);
    }
}
