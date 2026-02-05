use super::predictor::MLPredictor;
use crate::domain::trading::types::FeatureSet;
use smartcore::ensemble::random_forest_regressor::RandomForestRegressor;
use smartcore::linalg::basic::matrix::DenseMatrix;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use tracing::{error, info, warn};

pub struct SmartCorePredictor {
    model: Option<RandomForestRegressor<f64, f64, DenseMatrix<f64>, Vec<f64>>>,
    model_path: PathBuf,
}

impl SmartCorePredictor {
    pub fn new(model_path: PathBuf) -> Self {
        let mut predictor = Self {
            model: None,
            model_path,
        };
        predictor.load_model();
        predictor
    }

    fn load_model(&mut self) {
        if !self.model_path.exists() {
            warn!(
                "ML Model file not found at {:?}. Predictor will return neutral.",
                self.model_path
            );
            return;
        }

        match File::open(&self.model_path) {
            Ok(mut file) => {
                let mut buffer = Vec::new();
                if let Err(e) = file.read_to_end(&mut buffer) {
                    error!("Failed to read model file: {}", e);
                    return;
                }

                // Smartcore deserialization (using serde_json now)
                match serde_json::from_reader(std::io::Cursor::new(&buffer)) {
                    Ok(model) => {
                        info!("Successfully loaded ML model from {:?}", self.model_path);
                        self.model = Some(model);
                    }
                    Err(e) => {
                        error!("Failed to deserialize ML model: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Failed to open model file: {}", e);
            }
        }
    }

    fn features_to_vec(&self, fs: &FeatureSet) -> Vec<f64> {
        crate::domain::ml::feature_registry::features_to_f64_vector(fs)
    }
}

impl MLPredictor for SmartCorePredictor {
    fn predict(&self, features: &FeatureSet) -> Result<f64, String> {
        if let Some(model) = &self.model {
            let input_vec = self.features_to_vec(features);
            let input_matrix = match DenseMatrix::from_2d_vec(&vec![input_vec]) {
                Ok(m) => m,
                Err(e) => return Err(format!("Matrix creation failed: {}", e)),
            };

            match model.predict(&input_matrix) {
                Ok(predictions) => {
                    if let Some(pred) = predictions.first() {
                        Ok(*pred)
                    } else {
                        Err("No prediction returned".to_string())
                    }
                }
                Err(e) => Err(format!("Prediction failed: {}", e)),
            }
        } else {
            Ok(0.5) // Neutral
        }
    }

    fn name(&self) -> &str {
        "SmartCore Random Forest"
    }

    fn version(&self) -> &str {
        "v1.0"
    }
}
