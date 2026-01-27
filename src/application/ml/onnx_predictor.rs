use super::predictor::MLPredictor;
use crate::domain::trading::types::FeatureSet;
use ort::session::Session;
use std::path::PathBuf;
use tracing::{error, info, warn};

use std::sync::Mutex;

pub struct OnnxPredictor {
    session: Option<Mutex<Session>>,
    model_path: PathBuf,
}

impl OnnxPredictor {
    pub fn new(model_path: PathBuf) -> Self {
        let mut predictor = Self {
            session: None,
            model_path,
        };
        predictor.load_model();
        predictor
    }

    fn load_model(&mut self) {
        if !self.model_path.exists() {
            warn!(
                "ONNX Model file not found at {:?}. Predictor will return neutral.",
                self.model_path
            );
            return;
        }

        match Session::builder() {
            Ok(builder) => match builder.commit_from_file(&self.model_path) {
                Ok(session) => {
                    info!("Successfully loaded ONNX model from {:?}", self.model_path);
                    self.session = Some(Mutex::new(session));
                }
                Err(e) => {
                    error!("Failed to load ONNX model: {}", e);
                }
            },
            Err(e) => {
                error!("Failed to create ONNX session builder: {}", e);
            }
        }
    }

    fn features_to_inputs(&self, fs: &FeatureSet) -> (Vec<usize>, Vec<f32>) {
        let features = vec![
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
        ];
        (vec![1, features.len()], features)
    }
}

impl MLPredictor for OnnxPredictor {
    fn predict(&self, features: &FeatureSet) -> Result<f64, String> {
        if let Some(session_mutex) = &self.session {
            let mut session = session_mutex
                .lock()
                .map_err(|e| format!("Mutex lock failed: {}", e))?;

            let (shape, data) = self.features_to_inputs(features);

            // In ort 2.0, (shape, data) implements OwnedTensorArrayData if shape is ToShape
            let input_value = ort::value::Value::from_array((shape.as_slice(), data))
                .map_err(|e| format!("Input value creation failed: {}", e))?;

            // SessionInputs can be created from a fixed-size array of Values
            let inputs = ort::inputs![input_value];

            match session.run(inputs) {
                Ok(outputs) => {
                    let output_value = outputs
                        .iter()
                        .next()
                        .map(|(_, v)| v)
                        .ok_or("No output found")?;
                    let data = output_value
                        .try_extract_tensor::<f32>()
                        .map_err(|e| e.to_string())?;
                    Ok(*data.1.iter().next().ok_or("Empty output")? as f64)
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            Ok(0.0) // Neutral return in regression mode
        }
    }

    fn name(&self) -> &str {
        "ONNX Runtime"
    }

    fn version(&self) -> &str {
        "v2.0 (ort)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::FeatureSet;
    use std::path::PathBuf;

    #[test]
    fn test_onnx_predictor_no_model() {
        let predictor = OnnxPredictor::new(PathBuf::from("non_existent.onnx"));
        let fs = FeatureSet::default();
        let result = predictor.predict(&fs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[test]
    fn test_features_to_inputs() {
        let predictor = OnnxPredictor::new(PathBuf::from("dummy.onnx"));
        let fs = FeatureSet {
            rsi: Some(65.0),
            adx: Some(25.0),
            ..Default::default()
        };

        let (shape, data) = predictor.features_to_inputs(&fs);
        assert_eq!(shape, vec![1, 15]);
        assert_eq!(data[0], 65.0); // RSI
        assert_eq!(data[14], 25.0); // ADX
    }
}
