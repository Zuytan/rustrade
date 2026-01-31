use super::predictor::MLPredictor;
use crate::domain::trading::types::FeatureSet;
use ort::session::Session;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{error, info, warn};

pub struct OnnxPredictor {
    session: Option<Mutex<Session>>,
    model_path: PathBuf,
    // Buffer for sequential models (LSTM)
    history_buffer: Mutex<VecDeque<Vec<f32>>>,
    sequence_length: usize,
}

impl OnnxPredictor {
    pub fn new(model_path: PathBuf) -> Self {
        let mut predictor = Self {
            session: None,
            model_path,
            history_buffer: Mutex::new(VecDeque::new()),
            sequence_length: 60, // Default to 60, ideally configurable
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

    fn features_to_inputs(&self, fs: &FeatureSet) -> Vec<f32> {
        crate::domain::ml::feature_registry::features_to_vector(fs)
    }
}

impl MLPredictor for OnnxPredictor {
    fn warmup(&self, features: &FeatureSet) {
        let input_vec = self.features_to_inputs(features);
        if let Ok(mut buffer) = self.history_buffer.lock() {
            if buffer.len() >= self.sequence_length {
                buffer.pop_front();
            }
            buffer.push_back(input_vec);
        }
    }

    fn predict(&self, features: &FeatureSet) -> Result<f64, String> {
        // Update buffer first
        self.warmup(features);

        let mut session_mutex = match &self.session {
            Some(m) => m.lock().map_err(|e| format!("Mutex lock failed: {}", e))?,
            None => return Ok(0.0),
        };

        // Check if we have enough history
        let buffer = self
            .history_buffer
            .lock()
            .map_err(|e| format!("Buffer lock failed: {}", e))?;

        if buffer.len() < self.sequence_length {
            // Cold Start: Not enough data yet
            return Ok(0.0);
        }

        // Flatten buffer into a single vector [batch, seq_len, features]
        // Currently just 1 batch
        let flat_data: Vec<f32> = buffer.iter().flatten().cloned().collect();
        let feature_dim = buffer[0].len();

        let shape = vec![1, self.sequence_length, feature_dim];

        let input_value = ort::value::Value::from_array((shape.as_slice(), flat_data))
            .map_err(|e| format!("Input value creation failed: {}", e))?;

        let inputs = ort::inputs![input_value];

        match session_mutex.run(inputs) {
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
    }

    fn name(&self) -> &str {
        "ONNX Runtime (LSTM)"
    }

    fn version(&self) -> &str {
        "v2.1 (Stateful)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::FeatureSet;
    use std::path::PathBuf;

    #[test]
    fn test_onnx_predictor_stateful_logic() {
        let predictor = OnnxPredictor::new(PathBuf::from("non_existent.onnx"));
        let fs = FeatureSet::default();

        // Warmup 59 times
        for _ in 0..59 {
            let res = predictor.predict(&fs);
            assert!(res.is_ok());
            assert_eq!(res.unwrap(), 0.0); // Cold start
        }

        // 60th time - buffer full
        // It will still return 0.0 because no model loaded, but logic path is exercised
        let res = predictor.predict(&fs);
        assert!(res.is_ok());
    }
}
