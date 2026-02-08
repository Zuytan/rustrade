use clap::Parser;
use serde::Deserialize;
use smartcore::ensemble::random_forest_regressor::{
    RandomForestRegressor, RandomForestRegressorParameters,
};
use smartcore::linalg::basic::matrix::DenseMatrix;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TrainingRecord {
    timestamp: i64,
    symbol: String,
    rsi: f64,
    macd: f64,
    macd_signal: f64,
    macd_hist: f64,
    bb_width: f64,
    bb_position: f64,
    atr_pct: f64,
    hurst: f64,
    skewness: f64,
    momentum_norm: f64,
    volatility: f64,
    ofi: f64,
    cumulative_delta: f64,
    spread_bps: f64,
    adx: f64,
    return_1m: Option<f64>,
    return_5m: Option<f64>,
    return_15m: Option<f64>,
}

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

    /// Disable train/test split (train on 100% of data). Use after validation.
    #[arg(long)]
    no_split: bool,

    /// Time-series cross-validation folds (e.g. 5). When > 1, reports OOS mean and std; rejects if std > 50% of mean.
    #[arg(long, default_value_t = 0)]
    cv_folds: usize,

    /// Maximum number of rows to use (most recent). 0 = use all.
    #[arg(long, default_value_t = 0)]
    max_rows: usize,

    /// ML threshold for signal generation (used for evaluation metrics)
    #[arg(long, default_value_t = 0.0005)]
    threshold: f64,
}

/// Prints detailed prediction distribution analysis
fn print_prediction_analysis(predictions: &[f64], actuals: &[f64], threshold: f64) {
    let n = predictions.len();
    if n == 0 {
        return;
    }

    println!("\n══════════════════════════════════════════════════════");
    println!("  PREDICTION DISTRIBUTION ANALYSIS");
    println!("══════════════════════════════════════════════════════");

    // Prediction statistics
    let pred_mean = predictions.iter().sum::<f64>() / n as f64;
    let pred_min = predictions.iter().cloned().fold(f64::INFINITY, f64::min);
    let pred_max = predictions
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let pred_std = (predictions
        .iter()
        .map(|p| (p - pred_mean).powi(2))
        .sum::<f64>()
        / n as f64)
        .sqrt();

    println!("\n  Predictions (n={}):", n);
    println!("    Mean:   {:.6}  ({:.4}%)", pred_mean, pred_mean * 100.0);
    println!("    StdDev: {:.6}  ({:.4}%)", pred_std, pred_std * 100.0);
    println!("    Min:    {:.6}  ({:.4}%)", pred_min, pred_min * 100.0);
    println!("    Max:    {:.6}  ({:.4}%)", pred_max, pred_max * 100.0);

    // Signal distribution
    let buy_signals = predictions.iter().filter(|&&p| p > threshold).count();
    let sell_signals = predictions.iter().filter(|&&p| p < -threshold).count();
    let neutral = n - buy_signals - sell_signals;

    println!(
        "\n  Signal Distribution (threshold={:.4}%):",
        threshold * 100.0
    );
    println!(
        "    BUY  (>{:+.4}%): {:>7} ({:.1}%)",
        threshold * 100.0,
        buy_signals,
        buy_signals as f64 / n as f64 * 100.0
    );
    println!(
        "    SELL (<{:+.4}%): {:>7} ({:.1}%)",
        -threshold * 100.0,
        sell_signals,
        sell_signals as f64 / n as f64 * 100.0
    );
    println!(
        "    NEUTRAL:        {:>7} ({:.1}%)",
        neutral,
        neutral as f64 / n as f64 * 100.0
    );

    // Directional accuracy
    let mut correct_direction = 0;
    let mut correct_buy = 0;
    let mut total_buy = 0;
    let mut correct_sell = 0;
    let mut total_sell = 0;
    let mut profitable_buy = 0;
    let mut profitable_sell = 0;

    for (pred, actual) in predictions.iter().zip(actuals.iter()) {
        if (*pred > 0.0 && *actual > 0.0) || (*pred < 0.0 && *actual < 0.0) {
            correct_direction += 1;
        }
        if *pred > threshold {
            total_buy += 1;
            if *actual > 0.0 {
                correct_buy += 1;
            }
            if *actual > threshold {
                profitable_buy += 1;
            }
        }
        if *pred < -threshold {
            total_sell += 1;
            if *actual < 0.0 {
                correct_sell += 1;
            }
            if *actual < -threshold {
                profitable_sell += 1;
            }
        }
    }

    println!("\n  Directional Accuracy:");
    println!(
        "    Overall:    {:.1}%  ({}/{})",
        correct_direction as f64 / n as f64 * 100.0,
        correct_direction,
        n
    );
    if total_buy > 0 {
        println!(
            "    Buy  Win%:  {:.1}%  ({}/{})  | Profitable: {:.1}%",
            correct_buy as f64 / total_buy as f64 * 100.0,
            correct_buy,
            total_buy,
            profitable_buy as f64 / total_buy as f64 * 100.0
        );
    }
    if total_sell > 0 {
        println!(
            "    Sell Win%:  {:.1}%  ({}/{})  | Profitable: {:.1}%",
            correct_sell as f64 / total_sell as f64 * 100.0,
            correct_sell,
            total_sell,
            profitable_sell as f64 / total_sell as f64 * 100.0
        );
    }

    // Prediction histogram (10 buckets)
    println!("\n  Prediction Histogram:");
    let range = pred_max - pred_min;
    if range > 0.0 {
        let n_buckets = 10;
        let bucket_size = range / n_buckets as f64;
        let mut buckets = vec![0usize; n_buckets];
        for p in predictions {
            let idx = ((p - pred_min) / bucket_size).floor() as usize;
            let idx = idx.min(n_buckets - 1);
            buckets[idx] += 1;
        }
        let max_count = *buckets.iter().max().unwrap_or(&1);
        for (i, count) in buckets.iter().enumerate() {
            let lo = pred_min + i as f64 * bucket_size;
            let hi = lo + bucket_size;
            let bar_len = (*count as f64 / max_count as f64 * 40.0).ceil() as usize;
            let bar: String = "█".repeat(bar_len);
            println!(
                "    [{:+.5}% .. {:+.5}%] {:>7} {}",
                lo * 100.0,
                hi * 100.0,
                count,
                bar
            );
        }
    }

    println!("══════════════════════════════════════════════════════\n");
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
        let record: TrainingRecord = result?;

        if let Some(ret) = record.return_5m {
            // Features must match SmartCorePredictor::features_to_vec exactly!
            // Which now matches feature_registry order.
            // We manually reconstruct the vector here.
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

    // Apply --max-rows: keep only the most recent rows
    if args.max_rows > 0 && x.len() > args.max_rows {
        let skip = x.len() - args.max_rows;
        x.drain(..skip);
        y.drain(..skip);
        println!(
            "Using most recent {} rows (skipped {} older rows)",
            args.max_rows, skip
        );
    }

    let n = x.len();

    // Print target distribution
    let y_mean = y.iter().sum::<f64>() / n as f64;
    let y_pos = y.iter().filter(|&&v| v > 0.0).count();
    let y_buy = y.iter().filter(|&&v| v > args.threshold).count();
    let y_sell = y.iter().filter(|&&v| v < -args.threshold).count();
    println!("\nTarget Distribution (return_5m):");
    println!("  Total:    {}", n);
    println!("  Mean:     {:.6} ({:.4}%)", y_mean, y_mean * 100.0);
    println!(
        "  Positive: {} ({:.1}%)",
        y_pos,
        y_pos as f64 / n as f64 * 100.0
    );
    println!(
        "  Buy  (>{:+.4}%): {} ({:.1}%)",
        args.threshold * 100.0,
        y_buy,
        y_buy as f64 / n as f64 * 100.0
    );
    println!(
        "  Sell (<{:+.4}%): {} ({:.1}%)",
        -args.threshold * 100.0,
        y_sell,
        y_sell as f64 / n as f64 * 100.0
    );
    println!();

    if args.cv_folds > 1 {
        // Time-series CV: expanding train, test with gap. Fold i: train [0..train_end_i], test [test_start_i..test_end_i] with 5% gap.
        let gap_pct = 0.05;
        let mut oos_rmse = Vec::with_capacity(args.cv_folds);
        for fold in 0..args.cv_folds {
            let test_region_start =
                (n as f64 * (0.2 + (fold as f64 / args.cv_folds as f64) * 0.6)).floor() as usize;
            let test_region_end = (n as f64
                * (0.2 + ((fold + 1) as f64 / args.cv_folds as f64) * 0.6))
                .floor() as usize;
            let gap = (n as f64 * gap_pct).floor() as usize;
            let train_end = test_region_start.saturating_sub(gap).min(n);
            let test_start = test_region_start;
            let test_end = test_region_end.min(n);
            if train_end < 10 || test_end <= test_start {
                continue;
            }
            let x_train: Vec<Vec<f64>> = x[..train_end].to_vec();
            let y_train: Vec<f64> = y[..train_end].to_vec();
            let x_test: Vec<Vec<f64>> = x[test_start..test_end].to_vec();
            let y_test: Vec<f64> = y[test_start..test_end].to_vec();
            let x_train_m =
                DenseMatrix::from_2d_vec(&x_train).map_err(|e| format!("Matrix error: {}", e))?;
            let params = RandomForestRegressorParameters::default()
                .with_n_trees(args.n_trees)
                .with_max_depth(args.max_depth)
                .with_min_samples_split(args.min_split);
            let model = RandomForestRegressor::fit(&x_train_m, &y_train, params)
                .map_err(|e| format!("Training error: {}", e))?;
            let x_test_m =
                DenseMatrix::from_2d_vec(&x_test).map_err(|e| format!("Matrix error: {}", e))?;
            let pred: Vec<f64> = model
                .predict(&x_test_m)
                .map_err(|e| format!("Predict error: {}", e))?;
            let sq_err: f64 = pred
                .iter()
                .zip(y_test.iter())
                .map(|(p, t)| (p - t).powi(2))
                .sum();
            let rmse = (sq_err / pred.len() as f64).sqrt();
            oos_rmse.push(rmse);
        }
        if oos_rmse.is_empty() {
            println!("CV: No valid folds.");
        } else {
            let mean_rmse = oos_rmse.iter().sum::<f64>() / oos_rmse.len() as f64;
            let variance = oos_rmse
                .iter()
                .map(|r| (r - mean_rmse).powi(2))
                .sum::<f64>()
                / (oos_rmse.len() as f64 - 1.0).max(1.0);
            let std_rmse = variance.sqrt();
            println!("CV OOS RMSE: mean={:.6}, std={:.6}", mean_rmse, std_rmse);
            if mean_rmse > 0.0 && std_rmse > 0.5 * mean_rmse {
                println!(
                    "WARNING: Model unstable (std > 50% of mean). Consider more data or simpler model."
                );
            }
        }
        // Train final model on full data when using CV (or last 80% for consistency)
        let train_size = if args.no_split {
            n
        } else {
            (n as f64 * 0.8) as usize
        };
        let x_final: Vec<Vec<f64>> = x[..train_size].to_vec();
        let y_final: Vec<f64> = y[..train_size].to_vec();
        let x_matrix =
            DenseMatrix::from_2d_vec(&x_final).map_err(|e| format!("Matrix error: {}", e))?;
        let params = RandomForestRegressorParameters::default()
            .with_n_trees(args.n_trees)
            .with_max_depth(args.max_depth)
            .with_min_samples_split(args.min_split);
        let model = RandomForestRegressor::fit(&x_matrix, &y_final, params)
            .map_err(|e| format!("Training error: {}", e))?;
        if let Some(parent) = model_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut file = File::create(&model_path)?;
        serde_json::to_writer(&mut file, &model)?;
        println!(
            "Final model (trained on {} samples) saved to {:?}.",
            train_size, model_path
        );
        return Ok(());
    }

    let (x_train, y_train, x_test, y_test) = if args.no_split {
        (
            x.clone(),
            y.clone(),
            Vec::<Vec<f64>>::new(),
            Vec::<f64>::new(),
        )
    } else {
        let split = (n as f64 * 0.8).floor() as usize;
        let x_train = x[..split].to_vec();
        let y_train = y[..split].to_vec();
        let x_test = x[split..].to_vec();
        let y_test = y[split..].to_vec();
        (x_train, y_train, x_test, y_test)
    };

    println!("Training on {} samples...", x_train.len());
    let x_matrix =
        DenseMatrix::from_2d_vec(&x_train).map_err(|e| format!("Matrix error: {}", e))?;

    let params = RandomForestRegressorParameters::default()
        .with_n_trees(args.n_trees)
        .with_max_depth(args.max_depth)
        .with_min_samples_split(args.min_split);

    println!(
        "Training Random Forest Regressor (Trees: {}, Depth: {}, MinSplit: {})...",
        args.n_trees, args.max_depth, args.min_split
    );

    let model = RandomForestRegressor::fit(&x_matrix, &y_train, params.clone())
        .map_err(|e| format!("Training error: {}", e))?;

    if !x_test.is_empty() {
        let x_test_m =
            DenseMatrix::from_2d_vec(&x_test).map_err(|e| format!("Matrix error: {}", e))?;
        let pred: Vec<f64> = model
            .predict(&x_test_m)
            .map_err(|e| format!("Predict error: {}", e))?;
        let sq_err: f64 = pred
            .iter()
            .zip(y_test.iter())
            .map(|(p, t)| (p - t).powi(2))
            .sum();
        let rmse = (sq_err / pred.len() as f64).sqrt();
        let mae: f64 = pred
            .iter()
            .zip(y_test.iter())
            .map(|(p, t)| (p - t).abs())
            .sum::<f64>()
            / pred.len() as f64;
        let mean_y = y_test.iter().sum::<f64>() / y_test.len() as f64;
        let var_y: f64 =
            y_test.iter().map(|t| (t - mean_y).powi(2)).sum::<f64>() / y_test.len() as f64;
        let r2 = if var_y > 0.0 {
            1.0 - (sq_err / pred.len() as f64) / var_y
        } else {
            0.0
        };
        println!(
            "OOS Test (n={}): RMSE={:.6}, MAE={:.6}, R²={:.4}",
            x_test.len(),
            rmse,
            mae,
            r2
        );

        // Detailed prediction distribution analysis
        print_prediction_analysis(&pred, &y_test, args.threshold);
    }

    println!("Saving model to {:?}", model_path);
    if let Some(parent) = model_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut file = File::create(model_path)?;
    serde_json::to_writer(&mut file, &model)?;

    println!("Done. Model saved successfully.");
    Ok(())
}
