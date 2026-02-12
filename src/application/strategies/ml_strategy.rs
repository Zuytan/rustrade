use crate::application::ml::predictor::MLPredictor;
use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::trading::types::FeatureSet;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;

/// Explicit prediction mode for ML Strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionMode {
    /// Classification: score is a probability [0, 1]
    Classification,
    /// Regression: score is predicted return (e.g., 0.001 = 0.1%)
    Regression,
}

pub struct MLStrategy {
    predictor: Arc<Box<dyn MLPredictor>>,
    threshold: f64,
    mode: PredictionMode,
}

impl MLStrategy {
    pub fn new(predictor: Arc<Box<dyn MLPredictor>>, threshold: f64) -> Self {
        // Backwards-compatible: infer mode from threshold,
        // but prefer using with_mode() for clarity
        let mode = if threshold > 0.1 {
            PredictionMode::Classification
        } else {
            PredictionMode::Regression
        };
        Self {
            predictor,
            threshold,
            mode,
        }
    }

    /// Create with explicit prediction mode (preferred)
    pub fn with_mode(
        predictor: Arc<Box<dyn MLPredictor>>,
        threshold: f64,
        mode: PredictionMode,
    ) -> Self {
        Self {
            predictor,
            threshold,
            mode,
        }
    }

    fn extract_features(&self, ctx: &AnalysisContext) -> FeatureSet {
        if let Some(fs) = &ctx.feature_set {
            fs.clone()
        } else {
            FeatureSet {
                rsi: ctx.rsi,
                macd_line: ctx.macd_value,
                macd_signal: ctx.macd_signal,
                macd_hist: ctx.macd_histogram,
                bb_upper: ctx.bb_upper,
                bb_lower: ctx.bb_lower,
                bb_middle: ctx.bb_middle,
                atr: ctx.atr,
                hurst_exponent: ctx.hurst_exponent,
                skewness: ctx.skewness,
                momentum_normalized: ctx.momentum_normalized,
                realized_volatility: ctx.realized_volatility,
                ofi: Some(ctx.ofi_value),
                cumulative_delta: Some(ctx.cumulative_delta),
                ..Default::default()
            }
        }
    }
}

impl TradingStrategy for MLStrategy {
    fn warmup(&self, ctx: &AnalysisContext) {
        let features = self.extract_features(ctx);
        self.predictor.warmup(&features);
    }

    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let features = self.extract_features(ctx);

        match self.predictor.predict(&features) {
            Ok(score) => {
                // Regression Logic: score is predicted return (e.g., 0.001)
                // Threshold is minimum expected return (e.g., 0.0005)

                // Use explicit mode instead of fragile threshold-based heuristic
                let is_probability_mode = self.mode == PredictionMode::Classification;

                if is_probability_mode {
                    // Probability Mode (Classification)
                    if score > self.threshold {
                        Some(
                            Signal::buy(format!("ML Score {:.2} > {:.2}", score, self.threshold))
                                .with_confidence(score),
                        )
                    } else if score < (1.0 - self.threshold) {
                        Some(
                            Signal::sell(format!(
                                "ML Score {:.2} < {:.2}",
                                score,
                                1.0 - self.threshold
                            ))
                            .with_confidence(1.0 - score),
                        )
                    } else {
                        None
                    }
                } else {
                    // Regression Mode (Return Prediction). Confidence scales with prediction magnitude.
                    let score_dec = Decimal::from_f64_retain(score).unwrap_or(Decimal::ZERO);
                    let threshold_dec =
                        Decimal::from_f64_retain(self.threshold).unwrap_or(Decimal::ZERO);

                    let scale = (self.threshold * 10.0).max(0.001);
                    let magnitude = score.abs() / scale;
                    let confidence = (0.5 + (magnitude * 0.3)).min(0.95);

                    if score_dec > threshold_dec {
                        Some(
                            Signal::buy(format!(
                                "ML Pred Return {}% > {}%",
                                score_dec * dec!(100.0),
                                threshold_dec * dec!(100.0)
                            ))
                            .with_confidence(confidence),
                        )
                    } else if score_dec < -threshold_dec {
                        Some(
                            Signal::sell(format!(
                                "ML Pred Return {}% < -{}%",
                                score_dec * dec!(100.0),
                                threshold_dec * dec!(100.0)
                            ))
                            .with_confidence(confidence),
                        )
                    } else {
                        None
                    }
                }
            }
            Err(_) => None,
        }
    }

    fn name(&self) -> &str {
        "ML Strategy"
    }
}
