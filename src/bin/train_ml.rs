use clap::Parser;
use rustrade::application::ml::data_collector::TrainingDataPoint;
use smartcore::ensemble::random_forest_regressor::{
    RandomForestRegressor, RandomForestRegressorParameters,
};
use smartcore::linalg::basic::matrix::DenseMatrix;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to training data CSV
    #[arg(long, default_value = "data/ml/training_data.csv")]
    input: PathBuf,

    /// Path to output model file
    #[arg(long, default_value = "data/ml/model.bin")]
    output: PathBuf,

    /// Number of trees in the random forest
    #[arg(long, default_value_t = 100)]
    n_trees: usize,

    /// Maximum depth of trees
    #[arg(long, default_value_t = 10)]
    max_depth: u16,

    /// Minimum samples required to split an internal node
    #[arg(long, default_value_t = 5)]
    min_split: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let training_data_path = args.input;
    let model_path = args.output;

    if !training_data_path.exists() {
        println!(
            "Training data not found at {:?}. Please run bot with enable_ml_data_collection=true to generate data.",
            training_data_path
        );
        return Ok(());
    }

    println!("Loading training data from {:?}", training_data_path);
    let file = File::open(&training_data_path)?;
    let mut rdr = csv::Reader::from_reader(BufReader::new(file));

    let mut x: Vec<Vec<f64>> = Vec::new();
    let mut y: Vec<f64> = Vec::new();

    for result in rdr.deserialize() {
        let record: TrainingDataPoint = result?;

        if let Some(ret) = record.return_5m {
            // Features must match SmartCorePredictor::features_to_vec exactly!
            let features = vec![
                record.rsi,
                record.macd,
                record.macd_signal,
                record.macd_hist,
                record.bb_width,
                record.bb_position,
                record.atr_pct,
                record.hurst,
                record.skewness,
                record.momentum_norm,
                record.volatility,
                record.ofi,
                record.cumulative_delta,
                record.spread_bps,
                record.adx,
            ];

            x.push(features);
            y.push(ret); // Train directly on returns (Regression)
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

    let params = RandomForestRegressorParameters::default()
        .with_n_trees(args.n_trees as usize)
        .with_max_depth(args.max_depth as u16)
        .with_min_samples_split(args.min_split as usize);

    println!(
        "Training Random Forest Regressor (Trees: {}, Depth: {}, MinSplit: {})...",
        args.n_trees, args.max_depth, args.min_split
    );

    let model = RandomForestRegressor::fit(&x_matrix, &y, params)
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
