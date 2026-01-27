use super::{AnalysisContext, Signal, TradingStrategy};
use crate::application::ml::predictor::MLPredictor;
use crate::domain::trading::types::FeatureSet;
use std::sync::Arc;

pub struct MLStrategy {
    predictor: Arc<Box<dyn MLPredictor>>,
    threshold: f64,
}

impl MLStrategy {
    pub fn new(predictor: Arc<Box<dyn MLPredictor>>, threshold: f64) -> Self {
        Self {
            predictor,
            threshold,
        }
    }
}

impl TradingStrategy for MLStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // Construct FeatureSet from ctx context (simplified, ideally passed directly)
        // Since AnalysisContext deconstructs FeatureSet, we reconstruct what we need or modify analyze signature.
        // But AnalysisContext has the raw fields.
        // Use full FeatureSet if available (propagated from SignalGenerator)
        // Otherwise fallback to reconstruction (for backward campatibility or tests)
        let features = if let Some(fs) = &ctx.feature_set {
            fs.clone()
        } else {
            FeatureSet {
                rsi: Some(ctx.rsi),
                macd_line: Some(ctx.macd_value),
                macd_signal: Some(ctx.macd_signal),
                macd_hist: Some(ctx.macd_histogram),
                bb_upper: Some(ctx.bb_upper),
                bb_lower: Some(ctx.bb_lower),
                bb_middle: Some(ctx.bb_middle),
                atr: Some(ctx.atr),
                hurst_exponent: ctx.hurst_exponent,
                skewness: ctx.skewness,
                momentum_normalized: ctx.momentum_normalized,
                realized_volatility: ctx.realized_volatility,
                ..Default::default()
            }
        };

        match self.predictor.predict(&features) {
            Ok(score) => {
                // Regression Logic: score is predicted return (e.g., 0.001)
                // Threshold is minimum expected return (e.g., 0.0005)

                // If the threshold is large (> 0.1), we assume it's probability mode (legacy)
                // If it's small (< 0.1), we assume it's regression/return mode
                let is_probability_mode = self.threshold > 0.1;

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
                    // Regression Mode (Return Prediction)
                    if score > self.threshold {
                        Some(
                            Signal::buy(format!(
                                "ML Pred Return {:.4}% > {:.4}%",
                                score * 100.0,
                                self.threshold * 100.0
                            ))
                            .with_confidence(0.8), // Fixed confidence for now, or scale by magnitude
                        )
                    } else if score < -self.threshold {
                        Some(
                            Signal::sell(format!(
                                "ML Pred Return {:.4}% < -{:.4}%",
                                score * 100.0,
                                self.threshold * 100.0
                            ))
                            .with_confidence(0.8),
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
