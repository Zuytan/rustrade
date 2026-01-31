use crate::domain::trading::types::FeatureSet;

/// Interface for Machine Learning models
pub trait MLPredictor: Send + Sync {
    /// Predict probability/score (0.0 to 1.0)
    /// > 0.5 usually implies Up/Buy
    fn predict(&self, features: &FeatureSet) -> Result<f64, String>;

    /// Warmup the model state (e.g. history buffer for LSTMs)
    fn warmup(&self, _features: &FeatureSet) {}

    /// Get model name/type
    fn name(&self) -> &str;

    /// Get model version/id
    fn version(&self) -> &str;
}
