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
    use rust_decimal::prelude::ToPrimitive;
    let to_f32 = |opt: Option<rust_decimal::Decimal>, default: f64| {
        opt.and_then(|d| d.to_f32()).unwrap_or(default as f32)
    };

    vec![
        to_f32(fs.rsi, 50.0),
        to_f32(fs.macd_line, 0.0),
        to_f32(fs.macd_signal, 0.0),
        to_f32(fs.macd_hist, 0.0),
        to_f32(fs.bb_width, 0.0),
        to_f32(fs.bb_position, 0.5),
        to_f32(fs.atr_pct, 0.0),
        to_f32(fs.hurst_exponent, 0.5),
        to_f32(fs.skewness, 0.0),
        to_f32(fs.momentum_normalized, 0.0),
        to_f32(fs.realized_volatility, 0.0),
        to_f32(fs.ofi, 0.0),
        to_f32(fs.cumulative_delta, 0.0),
        to_f32(fs.spread_bps, 0.0),
        to_f32(fs.adx, 0.0),
    ]
}

/// Converts features into a vector of f64 for Data Collection (CSV).
/// Similar to `features_to_vector` but keeps f64 precision for training data.
pub fn features_to_f64_vector(fs: &FeatureSet) -> Vec<f64> {
    use rust_decimal::prelude::ToPrimitive;
    let to_f64 = |opt: Option<rust_decimal::Decimal>, default: f64| {
        opt.and_then(|d| d.to_f64()).unwrap_or(default)
    };

    vec![
        to_f64(fs.rsi, 50.0),
        to_f64(fs.macd_line, 0.0),
        to_f64(fs.macd_signal, 0.0),
        to_f64(fs.macd_hist, 0.0),
        to_f64(fs.bb_width, 0.0),
        to_f64(fs.bb_position, 0.5),
        to_f64(fs.atr_pct, 0.0),
        to_f64(fs.hurst_exponent, 0.5),
        to_f64(fs.skewness, 0.0),
        to_f64(fs.momentum_normalized, 0.0),
        to_f64(fs.realized_volatility, 0.0),
        to_f64(fs.ofi, 0.0),
        to_f64(fs.cumulative_delta, 0.0),
        to_f64(fs.spread_bps, 0.0),
        to_f64(fs.adx, 0.0),
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
        use rust_decimal_macros::dec;
        let fs = FeatureSet {
            rsi: Some(dec!(70.0)),
            adx: Some(dec!(25.0)),
            ..Default::default()
        };

        let vec = features_to_f64_vector(&fs);
        // RSI is index 0
        assert_eq!(vec[0], 70.0);
        // ADX is last index (14)
        assert_eq!(vec[14], 25.0);
    }
}
