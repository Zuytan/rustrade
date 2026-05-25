//! Derivative-based Spike Encoding for SNN Training.
//!
//! Converts raw price/ATR time series into multi-feature spike trains over time.
//! The canonical encoder is `encode_multi_feature` (v2.1, 10 features).
//! All production code uses `FeatureInputs` / `EncodedFeatures` for type safety.

use ndarray::Array1;

// Constants for feature encoding heuristics
const VOL_PERIOD: usize = 20;
const VOL_SPIKE_THRESHOLD: f64 = 2.0;
const VOL_SPIKE_MAX: f64 = 5.0;
const EMA_DIST_THRESHOLD: f64 = 3.0;
const EMA_DIST_MAX: f64 = 10.0;
const BBW_THRESHOLD: f64 = 0.10;
const BBW_MULTIPLIER: f64 = 20.0;
const VWAP_DIST_THRESHOLD: f64 = 2.0;
const VWAP_DIST_MAX: f64 = 5.0;
const MARKET_PULSE_VALUE: f64 = 0.2;

/// Struct holding encoded spike channels for SNN input.
///
/// Only `pos_channel` and `neg_channel` are used at inference time.
/// The additional fields (volumes, ema200s, etc.) were removed as they
/// were populated but never consumed downstream.
#[derive(Debug, Clone)]
pub struct EncodedFeatures {
    /// Positive (bullish) spike channel: Vec of [10-feature] arrays.
    pub pos_channel: Vec<Array1<f64>>,
    /// Negative (bearish) spike channel: Vec of [10-feature] arrays.
    pub neg_channel: Vec<Array1<f64>>,
}

/// Struct holding input features for encoding.
#[derive(Debug, Clone)]
pub struct FeatureInputs<'a> {
    pub prices: &'a [f64],
    pub atrs: &'a [f64],
    pub volumes: &'a [f64],
    pub ema200s: &'a [f64],
    pub bb_widths: &'a [f64],
    pub vwaps: &'a [f64],
    pub rsi_values: &'a [f64],
    pub macd_hist: &'a [f64],
}

/// Extended multi-feature encoding for richer input representation (v2.1 - 10 features).
///
/// Features:
/// 0. Price derivative (Δprice / ATR)
/// 1. Momentum (sign persistence)
/// 2. Volatility change (ΔATR / ATR)
/// 3. RSI deviation (from 50)
/// 4. MACD Histogram (normalized)
/// 5. Volume Spike (Volume / EMA20)
/// 6. EMA 200 Distance ((Price - EMA200) / ATR)
/// 7. Bollinger Width (Upper-Lower / Middle)
/// 8. VWAP Distance ((Price - VWAP) / ATR)
/// 9. Market Pulse (ALWAYS ON)
pub fn encode_multi_feature(
    inputs: FeatureInputs,
    spike_threshold: f64,
    momentum_window: usize,
) -> EncodedFeatures {
    let prices = inputs.prices;
    let atrs = inputs.atrs;
    let volumes = inputs.volumes;
    let ema200s = inputs.ema200s;
    let bb_widths = inputs.bb_widths;
    let vwaps = inputs.vwaps;
    let rsi_values = inputs.rsi_values;
    let macd_hist = inputs.macd_hist;

    if prices.len() < 2 {
        return EncodedFeatures {
            pos_channel: Vec::new(),
            neg_channel: Vec::new(),
        };
    }

    let len = prices.len() - 1;
    let num_features = 10;
    let mut pos_channel = Vec::with_capacity(len);
    let mut neg_channel = Vec::with_capacity(len);

    // Pre-compute Volume EMA for spike detection
    let mut vol_ema = volumes[0];
    let mut vol_emas = Vec::with_capacity(volumes.len());
    let alpha = 2.0 / (VOL_PERIOD + 1) as f64;
    for &v in volumes {
        vol_ema = v * alpha + vol_ema * (1.0 - alpha);
        vol_emas.push(vol_ema);
    }

    let mut deltas = Vec::with_capacity(len);
    for i in 1..prices.len() {
        let atr = if i < atrs.len() && atrs[i] > 1e-10 {
            atrs[i]
        } else {
            1.0
        };
        deltas.push((prices[i] - prices[i - 1]) / atr);
    }

    for i in 0..len {
        let price_deriv = deltas[i];
        let atr = if i + 1 < atrs.len() { atrs[i + 1] } else { 1.0 };

        // 1. Momentum
        let m_start = i.saturating_sub(momentum_window);
        let momentum: f64 = if i > m_start {
            deltas[m_start..=i].iter().sum::<f64>() / (i - m_start + 1) as f64
        } else {
            0.0
        };

        // 2. Volatility change
        let vol_change = if i + 1 < atrs.len() && i + 2 < atrs.len() && atrs[i + 1] > 1e-10 {
            (atrs[i + 2] - atrs[i + 1]) / atrs[i + 1]
        } else {
            0.0
        };

        let mut pos = Array1::zeros(num_features);
        let mut neg = Array1::zeros(num_features);

        // F0: Price deriv
        if price_deriv > spike_threshold {
            pos[0] = price_deriv;
        } else if price_deriv < -spike_threshold {
            neg[0] = price_deriv.abs();
        }

        // F1: Momentum
        if momentum > spike_threshold * 0.5 {
            pos[1] = momentum;
        } else if momentum < -spike_threshold * 0.5 {
            neg[1] = momentum.abs();
        }

        // F2: Vol change
        if vol_change > spike_threshold * 0.3 {
            pos[2] = vol_change;
        } else if vol_change < -spike_threshold * 0.3 {
            neg[2] = vol_change.abs();
        }

        // F3: RSI (Spike when extreme)
        let rsi_dev = if i < rsi_values.len() {
            (rsi_values[i] - 50.0) / 50.0
        } else {
            0.0
        };
        if rsi_dev > spike_threshold * 0.4 {
            pos[3] = rsi_dev;
        } else if rsi_dev < -spike_threshold * 0.4 {
            neg[3] = rsi_dev.abs();
        }

        // F4: MACD
        let m_hist = if i < macd_hist.len() {
            macd_hist[i]
        } else {
            0.0
        };
        if m_hist > spike_threshold {
            pos[4] = m_hist;
        } else if m_hist < -spike_threshold {
            neg[4] = m_hist.abs();
        }

        // F5: Volume Spike [NEW]
        let v_spike = if i + 1 < volumes.len() && vol_emas[i + 1] > 1e-10 {
            (volumes[i + 1] / vol_emas[i + 1]) - 1.0
        } else {
            0.0
        };
        if v_spike > VOL_SPIKE_THRESHOLD {
            pos[5] = (v_spike - VOL_SPIKE_THRESHOLD).min(VOL_SPIKE_MAX);
        }

        // F6: EMA 200 Dist [NEW]
        let ema_dist = if i + 1 < ema200s.len() && atr > 1e-10 {
            (prices[i + 1] - ema200s[i + 1]) / atr
        } else {
            0.0
        };
        if ema_dist > EMA_DIST_THRESHOLD {
            pos[6] = (ema_dist - EMA_DIST_THRESHOLD).min(EMA_DIST_MAX);
        } else if ema_dist < -EMA_DIST_THRESHOLD {
            neg[6] = (ema_dist.abs() - EMA_DIST_THRESHOLD).min(EMA_DIST_MAX);
        }

        // F7: BB Width [NEW]
        let bbw = if i + 1 < bb_widths.len() {
            bb_widths[i + 1]
        } else {
            0.0
        };
        if bbw > BBW_THRESHOLD {
            pos[7] = (bbw - BBW_THRESHOLD) * BBW_MULTIPLIER;
        }

        // F8: VWAP Dist [NEW]
        let vwap_dist = if i + 1 < vwaps.len() && atr > 1e-10 {
            (prices[i + 1] - vwaps[i + 1]) / atr
        } else {
            0.0
        };
        if vwap_dist > VWAP_DIST_THRESHOLD {
            pos[8] = (vwap_dist - VWAP_DIST_THRESHOLD).min(VWAP_DIST_MAX);
        } else if vwap_dist < -VWAP_DIST_THRESHOLD {
            neg[8] = (vwap_dist.abs() - VWAP_DIST_THRESHOLD).min(VWAP_DIST_MAX);
        }

        // F9: Market Pulse (ALWAYS ON)
        pos[9] = MARKET_PULSE_VALUE;
        neg[9] = MARKET_PULSE_VALUE;

        pos_channel.push(pos);
        neg_channel.push(neg);
    }

    EncodedFeatures {
        pos_channel,
        neg_channel,
    }
}

/// Computes forward returns for label generation.
///
/// # Arguments
/// * `prices` - Close prices.
/// * `horizon` - Number of bars forward to measure return (e.g., 5 for 5-bar return).
///
/// # Returns
/// Vector of `(index, return)` pairs. Length = `prices.len() - horizon`.
pub fn compute_forward_returns(prices: &[f64], horizon: usize) -> Vec<(usize, f64)> {
    if prices.len() <= horizon {
        return Vec::new();
    }

    (0..prices.len() - horizon)
        .map(|i| {
            let ret = (prices[i + horizon] - prices[i]) / prices[i];
            (i, ret)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_feature_encoding_shape() {
        let prices = vec![100.0, 101.0, 102.0, 103.0, 104.0, 105.0];
        let atrs = vec![1.0; 6];
        let vol = vec![1000.0; 6];
        let ema = vec![100.0; 6];
        let bbw = vec![0.1; 6];
        let vwap = vec![100.0; 6];
        let rsi = vec![50.0; 6];
        let macd = vec![0.0; 6];
        let inputs = FeatureInputs {
            prices: &prices,
            atrs: &atrs,
            volumes: &vol,
            ema200s: &ema,
            bb_widths: &bbw,
            vwaps: &vwap,
            rsi_values: &rsi,
            macd_hist: &macd,
        };
        let features = encode_multi_feature(inputs, 0.5, 3);

        assert_eq!(features.pos_channel.len(), 5); // prices.len() - 1
        assert_eq!(features.neg_channel.len(), 5);
        assert_eq!(features.pos_channel[0].len(), 10); // 10 features v2.1
    }

    #[test]
    fn test_forward_returns() {
        let prices = vec![100.0, 102.0, 104.0, 103.0, 105.0];
        let returns = compute_forward_returns(&prices, 2);

        // Return at index 0: (104 - 100) / 100 = 0.04
        assert_eq!(returns.len(), 3);
        assert!((returns[0].1 - 0.04).abs() < 1e-10);
    }

    #[test]
    fn test_forward_returns_empty() {
        let returns = compute_forward_returns(&[100.0, 101.0], 5);
        assert!(returns.is_empty());
    }
}
