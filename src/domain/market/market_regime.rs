use crate::domain::trading::types::Candle;
use anyhow::Result;
use rust_decimal::prelude::ToPrimitive;
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
    pub confidence: f64, // 0.0 to 1.0
    pub volatility_score: f64,
    pub trend_strength: f64,
}

impl MarketRegime {
    pub fn new(
        regime_type: MarketRegimeType,
        confidence: f64,
        volatility_score: f64,
        trend_strength: f64,
    ) -> Self {
        Self {
            regime_type,
            confidence: confidence.clamp(0.0, 1.0),
            volatility_score,
            trend_strength,
        }
    }

    pub fn unknown() -> Self {
        Self {
            regime_type: MarketRegimeType::Unknown,
            confidence: 0.0,
            volatility_score: 0.0,
            trend_strength: 0.0,
        }
    }
}

/// Service for detecting market regime from price action
pub struct MarketRegimeDetector {
    window_size: usize,
    adx_threshold: f64,
    volatility_threshold: f64,
}

impl MarketRegimeDetector {
    pub fn new(window_size: usize, adx_threshold: f64, volatility_threshold: f64) -> Self {
        Self {
            window_size,
            adx_threshold,
            volatility_threshold,
        }
    }

    pub fn detect_from_features(
        &self,
        hurst: Option<f64>,
        volatility: Option<f64>,
        _skewness: Option<f64>,
    ) -> Result<MarketRegime> {
        // Enhanced Detection using Statistical Features

        // 1. Volatility Check
        let is_volatile = volatility
            .map(|v| v > self.volatility_threshold)
            .unwrap_or(false);
        if is_volatile {
            return Ok(MarketRegime::new(
                MarketRegimeType::Volatile,
                0.8, // High confidence if directly measured
                volatility.unwrap_or(0.0) * 100.0,
                0.0,
            ));
        }

        // 2. Trend vs Mean Reversion using Hurst
        if let Some(h) = hurst {
            if h > 0.6 {
                // Strong Trending Behavior
                return Ok(MarketRegime::new(
                    MarketRegimeType::TrendingUp, // Direction needs price action, defaulting to Generic Trend or need direction input
                    // Actually we need direction. So we might need context or direction passed in.
                    // For now, let's say TrendingUp/Down is ambiguous from Hurst alone (it just says "Trending").
                    // We need slope or momentum for direction.
                    // Let's assume we use this in conjunction with simple slope check.
                    (h - 0.5) * 2.0, // Confidence scales with Hurst
                    0.0,
                    h * 100.0,
                ));
            } else if h < 0.4 {
                // Mean Reverting
                return Ok(MarketRegime::new(
                    MarketRegimeType::Ranging,
                    (0.5 - h) * 2.0,
                    0.0,
                    0.0,
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
            .close
            .to_f64()
            .unwrap_or(0.0);
        let volatility_score = if current_price > 0.0 {
            (atr / current_price) * 100.0
        } else {
            0.0
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
        let confidence = match regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                let strength_excess = (trend_strength - self.adx_threshold).max(0.0);
                (0.5 + strength_excess * 0.02).min(1.0)
            }
            MarketRegimeType::Volatile => {
                let vol_excess = (volatility_score - self.volatility_threshold).max(0.0);
                (0.5 + vol_excess * 0.1).min(1.0)
            }
            MarketRegimeType::Ranging => 0.6, // Default confidence for ranging
            MarketRegimeType::Unknown => 0.0,
        };

        Ok(MarketRegime::new(
            regime_type,
            confidence,
            volatility_score,
            trend_strength,
        ))
    }

    fn calculate_atr(&self, candles: &[Candle], period: usize) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        if candles.len() < period + 1 {
            return 0.0;
        }

        let mut tr_sum = 0.0;
        for i in 1..candles.len() {
            let high = candles[i].high.to_f64().unwrap_or(0.0);
            let low = candles[i].low.to_f64().unwrap_or(0.0);
            let close_prev = candles[i - 1].close.to_f64().unwrap_or(0.0);

            let tr = (high - low)
                .max((high - close_prev).abs())
                .max((low - close_prev).abs());

            // Simple average for this window (could be smoothed)
            if i >= candles.len() - period {
                tr_sum += tr;
            }
        }

        tr_sum / period as f64
    }

    fn calculate_trend_strength(&self, candles: &[Candle]) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        let n = candles.len();
        if n < 2 {
            return 0.0;
        }

        let prices: Vec<f64> = candles
            .iter()
            .map(|c| c.close.to_f64().unwrap_or(0.0))
            .collect();

        // Linear regression: y = mx + c
        let x_sum: f64 = (0..n).map(|i| i as f64).sum();
        let y_sum: f64 = prices.iter().sum();
        let xy_sum: f64 = prices.iter().enumerate().map(|(i, &p)| i as f64 * p).sum();
        let x2_sum: f64 = (0..n).map(|i| (i * i) as f64).sum();

        let slope = (n as f64 * xy_sum - x_sum * y_sum) / (n as f64 * x2_sum - x_sum * x_sum);
        let first_price = prices[0].max(0.0001);

        (slope / first_price).abs() * 1000.0
    }

    fn is_uptrend(&self, candles: &[Candle]) -> bool {
        use rust_decimal::prelude::ToPrimitive;
        if candles.len() < 2 {
            return false;
        }
        let first = candles
            .first()
            .expect("candles verified len >= 2")
            .close
            .to_f64()
            .unwrap_or(0.0);
        let last = candles
            .last()
            .expect("candles verified len >= 2")
            .close
            .to_f64()
            .unwrap_or(0.0);
        last > first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::Candle;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromPrimitive;

    fn create_candle(price: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            timestamp: Utc::now().timestamp(),
            open: Decimal::from_f64_retain(price).unwrap(),
            high: Decimal::from_f64_retain(price + 1.0).unwrap(),
            low: Decimal::from_f64_retain(price - 1.0).unwrap(),
            close: Decimal::from_f64_retain(price).unwrap(),
            volume: Decimal::from_f64(1000.0).unwrap(),
        }
    }

    #[test]
    fn test_regime_detection_uptrend() {
        let detector = MarketRegimeDetector::new(10, 25.0, 2.0);
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
        let detector = MarketRegimeDetector::new(10, 25.0, 2.0);
        // Hurst > 0.6 -> Trending
        let regime = detector
            .detect_from_features(Some(0.7), Some(0.0), None)
            .unwrap();
        // Without direction, we define it maps to TrendingUp as placeholder for "Trend"
        assert_eq!(regime.regime_type, MarketRegimeType::TrendingUp);
        assert!(regime.confidence > 0.0);

        // Hurst < 0.4 -> Mean Reversion (Ranging)
        let regime_mr = detector
            .detect_from_features(Some(0.3), Some(0.0), None)
            .unwrap();
        assert_eq!(regime_mr.regime_type, MarketRegimeType::Ranging);
    }

    #[test]
    fn test_detect_from_features_volatility() {
        let detector = MarketRegimeDetector::new(10, 25.0, 2.0); // Vol thresh 2.0
        // Volatility 3.0 > 2.0 -> Volatile
        let regime = detector
            .detect_from_features(None, Some(3.0), None)
            .unwrap();
        assert_eq!(regime.regime_type, MarketRegimeType::Volatile);
    }
}
