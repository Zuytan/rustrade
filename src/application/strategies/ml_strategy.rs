use crate::application::ml::predictor::MLPredictor;
use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::trading::types::FeatureSet;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
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

    fn extract_features(&self, ctx: &AnalysisContext) -> FeatureSet {
        if let Some(fs) = &ctx.feature_set {
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
