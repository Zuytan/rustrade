use crate::domain::trading::types::FeatureSet;

/// Ordered list of feature names.
/// This order MUST match exactly with the order used in Python training scripts.
/// Any change here is a breaking change for ML models.
pub const FEATURE_NAMES: &[&str] = &[
    "rsi",
    "macd",
    "macd_signal",
    "macd_hist",
    "bb_width",
    "bb_position",
    "atr_pct",
    "hurst",
    "skewness",
    "momentum_norm",
    "volatility",
    "ofi",
    "cumulative_delta",
    "spread_bps",
    "adx",
];

/// Converts returns features into a normalized vector (f32) for ONNX inference.
/// Handles Option unwrapping with default fallbacks.
pub fn features_to_vector(fs: &FeatureSet) -> Vec<f32> {
    vec![
        fs.rsi.unwrap_or(50.0) as f32,
        fs.macd_line.unwrap_or(0.0) as f32,
        fs.macd_signal.unwrap_or(0.0) as f32,
        fs.macd_hist.unwrap_or(0.0) as f32,
        fs.bb_width.unwrap_or(0.0) as f32,
        fs.bb_position.unwrap_or(0.5) as f32,
        fs.atr_pct.unwrap_or(0.0) as f32,
        fs.hurst_exponent.unwrap_or(0.5) as f32,
        fs.skewness.unwrap_or(0.0) as f32,
        fs.momentum_normalized.unwrap_or(0.0) as f32,
        fs.realized_volatility.unwrap_or(0.0) as f32,
        fs.ofi.unwrap_or(0.0) as f32,
        fs.cumulative_delta.unwrap_or(0.0) as f32,
        fs.spread_bps.unwrap_or(0.0) as f32,
        fs.adx.unwrap_or(0.0) as f32,
    ]
}

/// Converts features into a vector of f64 for Data Collection (CSV).
/// Similar to `features_to_vector` but keeps f64 precision for training data.
pub fn features_to_f64_vector(fs: &FeatureSet) -> Vec<f64> {
    vec![
        fs.rsi.unwrap_or(50.0),
        fs.macd_line.unwrap_or(0.0),
        fs.macd_signal.unwrap_or(0.0),
        fs.macd_hist.unwrap_or(0.0),
        fs.bb_width.unwrap_or(0.0),
        fs.bb_position.unwrap_or(0.5),
        fs.atr_pct.unwrap_or(0.0),
        fs.hurst_exponent.unwrap_or(0.5),
        fs.skewness.unwrap_or(0.0),
        fs.momentum_normalized.unwrap_or(0.0),
        fs.realized_volatility.unwrap_or(0.0),
        fs.ofi.unwrap_or(0.0),
        fs.cumulative_delta.unwrap_or(0.0),
        fs.spread_bps.unwrap_or(0.0),
        fs.adx.unwrap_or(0.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::FeatureSet;

    #[test]
    fn test_feature_vector_length() {
        let fs = FeatureSet::default();
        let vec = features_to_vector(&fs);
        assert_eq!(vec.len(), FEATURE_NAMES.len());
    }

    #[test]
    fn test_feature_consistency() {
        let fs = FeatureSet {
            rsi: Some(70.0),
            adx: Some(25.0),
            ..Default::default()
        };

        let vec = features_to_f64_vector(&fs);
        // RSI is index 0
        assert_eq!(vec[0], 70.0);
        // ADX is last index (14)
        assert_eq!(vec[14], 25.0);
    }
}
