use rustrade::application::ml::data_collector::TrainingDataPoint;
use smartcore::ensemble::random_forest_classifier::{
    RandomForestClassifier, RandomForestClassifierParameters,
};
use smartcore::linalg::basic::matrix::DenseMatrix;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let training_data_path = PathBuf::from("data/ml/training_data.csv");
    let model_path = PathBuf::from("data/ml/model.bin");

    if !training_data_path.exists() {
        println!(
            "Training data not found at {:?}. Please run bot with enable_ml_data_collection=true to generate data.",
            training_data_path
        );
        return Ok(());
    }

    println!("Loading training data from {:?}", training_data_path);
    let file = File::open(training_data_path)?;
    let mut rdr = csv::Reader::from_reader(BufReader::new(file));

    let mut x: Vec<Vec<f64>> = Vec::new();
    let mut y: Vec<i32> = Vec::new();

    for result in rdr.deserialize() {
        let record: TrainingDataPoint = result?;

        if let Some(ret) = record.return_5m {
            // Labeling Logic: 0.05% threshold
            let label = if ret > 0.0005 {
                1 // Buy
            } else {
                0 // Sell/Hold - simplistic binary
            };

            // Features must match SmartCorePredictor::features_to_vec exactly!
            let features = vec![
                record.rsi,
                record.macd,
                record.macd_signal,
                record.macd_hist,
                0.0, // BB Width placeholder
                0.5, // BB Position placeholder
                0.0, // ATR Pct placeholder
                record.hurst,
                record.skewness,
                record.momentum_norm,
                record.volatility,
            ];

            x.push(features);
            y.push(label);
        }
    }

    if x.is_empty() {
        println!("No labeled data found.");
        return Ok(());
    }

    println!("Training on {} samples...", x.len());
    // Create DenseMatrix
    // Note: unwraping might panic if data is invalid, but for CLI tool it's acceptable for now
    let x_matrix = DenseMatrix::from_2d_vec(&x).map_err(|e| format!("Matrix error: {}", e))?;

    let params = RandomForestClassifierParameters::default()
        .with_n_trees(100)
        .with_max_depth(10)
        .with_min_samples_split(5);

    let model = RandomForestClassifier::fit(&x_matrix, &y, params)
        .map_err(|e| format!("Training error: {}", e))?;

    println!("Saving model to {:?}", model_path);
    // Ensure dir exists
    if let Some(parent) = model_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut file = File::create(model_path)?;
    bincode::serialize_into(&mut file, &model)?;

    println!("Done. Model saved successfully.");
    Ok(())
}
