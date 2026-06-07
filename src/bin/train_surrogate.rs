//! Surrogate Gradient SNN Training Binary.
//!
//! Trains a competitive Izhikevich SNN using BPTT with surrogate gradients
//! on historical market data. This replaces the genetic algorithm approach
//! with direct gradient-based optimization.
//!
//! # Usage
//! ```bash
//! cargo run --bin train_surrogate --features surrogate -- \
//!   --symbol BTC/USD --days 7 --epochs 50 --lr 0.001
//! ```

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use clap::Parser;
use ndarray::{Array1, Array2};
use rayon::prelude::*;
use rust_decimal::prelude::ToPrimitive;

use rustrade::application::ml::derivative_encoding::{
    FeatureInputs, compute_forward_returns, encode_multi_feature,
};
use rustrade::application::monitoring::feature_engineering_service::TechnicalFeatureEngineeringService;
use rustrade::domain::ports::{FeatureEngineeringService, MarketDataService};
use rustrade::domain::snn::competitive_network::CompetitiveSnnNetwork;
use rustrade::domain::snn::gradients::SnnGradients;
use rustrade::domain::snn::hyperparams::SnnHyperparameters;
use rustrade::domain::snn::loss::{
    ConfusionMatrix, PsaLossConfig, compute_class_weights, compute_target_rate, returns_to_labels,
};
use rustrade::domain::snn::optimizer::AdamOptimizer;
use rustrade::domain::snn::surrogate::SurrogateType;
use rustrade::infrastructure::alpaca::AlpacaMarketDataService;

use rustrade::application::agents::analyst_config::AnalystConfig;

#[derive(Parser, Debug)]
#[command(author, version, about = "Train SNN with Surrogate Gradient BPTT")]
struct Args {
    /// Trading symbol
    #[arg(short, long, default_value = "BTC/USD")]
    symbol: String,

    /// Days of historical data (if start/end not provided)
    #[arg(short, long, default_value = "30")]
    days: i64,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    start: Option<String>,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    end: Option<String>,

    /// Training epochs
    #[arg(short, long, default_value = "50")]
    epochs: usize,

    /// Mini-batch size for parallel processing
    #[arg(long, default_value = "32")]
    batch_size: usize,

    /// Early stopping patience
    #[arg(long, default_value = "15")]
    patience: usize,

    /// Early stopping grace period (epochs to ignore before tracking best loss)
    #[arg(long, default_value = "15")]
    grace_period: usize,

    /// Initial learning rate
    #[arg(long, default_value = "0.001")]
    lr: f64,

    /// Neurons in bullish pathway
    #[arg(long, default_value = "32")]
    hidden_a: usize,

    /// Neurons in bearish pathway
    #[arg(long, default_value = "32")]
    hidden_b: usize,

    /// Initial surrogate gradient steepness (β)
    #[arg(long, default_value = "1.0")]
    beta_start: f64,

    /// Final surrogate gradient steepness (β)
    #[arg(long, default_value = "20.0")]
    beta_end: f64,

    /// Surrogate function type (fast_sigmoid, atan, triangle, exponential)
    #[arg(long, default_value = "triangle")]
    surrogate_type: String,

    /// Izhikevich spike threshold (mV)
    #[arg(long, default_value = "30.0")]
    threshold: f64,

    /// Rate penalty multiplier (lambda_rate)
    #[arg(long, default_value = "0.1")]
    lambda_rate: f64,

    /// Silence penalty multiplier (lambda_silence)
    #[arg(long, default_value = "1.0")]
    lambda_silence: f64,

    /// Overtrade penalty multiplier (lambda_overtrade)
    #[arg(long, default_value = "0.5")]
    lambda_overtrade: f64,

    /// Derivative encoding spike threshold
    #[arg(long, default_value = "0.5")]
    spike_threshold: f64,

    /// Forward return horizon (bars) for label generation
    #[arg(long, default_value = "5")]
    return_horizon: usize,

    /// Minimum |return| to classify as buy/sell
    #[arg(long, default_value = "0.005")]
    label_threshold: f64,

    /// Data timeframe
    #[arg(long, default_value = "1Min")]
    timeframe: String,

    /// Optional: load hyperparams from JSON (for Bayesian optimization)
    #[arg(long)]
    hyperparams: Option<String>,

    /// Resume training from a saved model (full CompetitiveSnnNetwork JSON)
    #[arg(long)]
    resume: Option<String>,

    /// Output model path
    #[arg(short, long, default_value = "models/snn/snn_surrogate_model.json")]
    output: String,

    /// Train/validation split ratio
    #[arg(long, default_value = "0.8")]
    train_ratio: f64,

    /// Disable class weights (inverse frequency) in loss
    #[arg(long, default_value = "false")]
    no_class_weights: bool,

    /// L2 weight decay (decoupled, applied after each optimizer step)
    #[arg(long, default_value = "0.0001")]
    weight_decay: f64,

    /// Minimum learning rate for cosine annealing schedule
    #[arg(long, default_value = "0.00001")]
    lr_min: f64,
}

struct TrainingData {
    pos_inputs: Vec<Array1<f64>>,
    neg_inputs: Vec<Array1<f64>>,
    labels: Vec<usize>,
    atrs: Vec<f64>,
    global_mean_atr: f64,
    target_rate: f64,
    class_weights: Option<Array1<f64>>,
}

async fn fetch_and_encode_data(args: &Args) -> Result<TrainingData> {
    let api_key = std::env::var("ALPACA_API_KEY").context("ALPACA_API_KEY must be set")?;
    let api_secret = std::env::var("ALPACA_SECRET_KEY").context("ALPACA_SECRET_KEY must be set")?;
    let data_url = std::env::var("ALPACA_DATA_URL")
        .unwrap_or_else(|_| "https://data.alpaca.markets".to_string());
    let api_base_url = std::env::var("ALPACA_BASE_URL")
        .unwrap_or_else(|_| "https://paper-api.alpaca.markets".to_string());
    let ws_url = std::env::var("ALPACA_WS_URL")
        .unwrap_or_else(|_| "wss://stream.data.alpaca.markets/v2/iex".to_string());

    let market_service = AlpacaMarketDataService::builder()
        .api_key(api_key)
        .api_secret(api_secret)
        .data_base_url(data_url)
        .api_base_url(api_base_url)
        .ws_url(ws_url)
        .asset_class(rustrade::config::AssetClass::Crypto)
        .build();

    let end = if let Some(end_str) = &args.end {
        chrono::NaiveDate::parse_from_str(end_str, "%Y-%m-%d")?
            .and_hms_opt(23, 59, 59)
            .context("invalid end time")?
            .and_utc()
    } else {
        Utc::now()
    };

    let start = if let Some(start_str) = &args.start {
        chrono::NaiveDate::parse_from_str(start_str, "%Y-%m-%d")?
            .and_hms_opt(0, 0, 0)
            .context("invalid start time")?
            .and_utc()
    } else {
        end - Duration::days(args.days)
    };

    let bars = market_service
        .get_historical_bars(&args.symbol, start, end, &args.timeframe)
        .await?;
    tracing::info!(
        "Fetched {} bars from {} to {}",
        bars.len(),
        start.date_naive(),
        end.date_naive()
    );

    if bars.len() < 100 {
        anyhow::bail!("Not enough data: {} bars (need at least 100)", bars.len());
    }

    let config = AnalystConfig::default();
    let mut feature_service = TechnicalFeatureEngineeringService::new(&config);

    let mut prices = Vec::with_capacity(bars.len());
    let mut atrs = Vec::with_capacity(bars.len());
    let mut volumes = Vec::with_capacity(bars.len());
    let mut ema200s = Vec::with_capacity(bars.len());
    let mut bb_widths = Vec::with_capacity(bars.len());
    let mut vwaps = Vec::with_capacity(bars.len());
    let mut rsi_values = Vec::with_capacity(bars.len());
    let mut macd_hists = Vec::with_capacity(bars.len());

    for bar in &bars {
        let features = feature_service.update(bar);
        let price = bar.close.to_f64().context("invalid close price")?;
        prices.push(price);
        atrs.push(
            features
                .atr
                .and_then(|a| a.to_f64())
                .unwrap_or(1.0)
                .max(1e-10),
        );
        volumes.push(bar.volume.to_f64().context("invalid volume")?);
        ema200s.push(features.sma_200.and_then(|e| e.to_f64()).unwrap_or(price));
        bb_widths.push(features.bb_width.and_then(|b| b.to_f64()).unwrap_or(0.0));
        vwaps.push(features.vwap.and_then(|v| v.to_f64()).unwrap_or(price));
        rsi_values.push(features.rsi.and_then(|r| r.to_f64()).unwrap_or(50.0));
        macd_hists.push(features.macd_hist.and_then(|m| m.to_f64()).unwrap_or(0.0));
    }

    let encoded = encode_multi_feature(
        FeatureInputs {
            prices: &prices,
            atrs: &atrs,
            volumes: &volumes,
            ema200s: &ema200s,
            bb_widths: &bb_widths,
            vwaps: &vwaps,
            rsi_values: &rsi_values,
            macd_hist: &macd_hists,
        },
        args.spike_threshold,
        5,
    );

    tracing::info!(
        "Encoded {} timesteps with {} features per channel",
        encoded.pos_channel.len(),
        if encoded.pos_channel.is_empty() {
            0
        } else {
            encoded.pos_channel[0].len()
        }
    );

    let forward_returns = compute_forward_returns(&prices, args.return_horizon);
    let returns_for_labels: Vec<f64> = forward_returns.iter().map(|(_, r)| *r).collect();
    let labels = returns_to_labels(&returns_for_labels, args.label_threshold);
    let target_rate = compute_target_rate(&labels);

    let buy_count = labels.iter().filter(|&&l| l == 0).count();
    let sell_count = labels.iter().filter(|&&l| l == 1).count();
    let hold_count = labels.iter().filter(|&&l| l == 2).count();

    let class_weights = if !args.no_class_weights {
        let weights = compute_class_weights(&labels, 3);
        tracing::info!(
            "Class weights: buy={:.2}, sell={:.2}, hold={:.2}",
            weights[0],
            weights[1],
            weights[2]
        );
        Some(weights)
    } else {
        tracing::info!("Class weights disabled.");
        None
    };

    tracing::info!(
        "Labels: {} buy, {} sell, {} hold (target_rate={:.4})",
        buy_count,
        sell_count,
        hold_count,
        target_rate
    );

    let global_mean_atr = if atrs.is_empty() {
        1.0
    } else {
        atrs.iter().sum::<f64>() / atrs.len() as f64
    };

    Ok(TrainingData {
        pos_inputs: encoded.pos_channel,
        neg_inputs: encoded.neg_channel,
        labels,
        atrs,
        global_mean_atr,
        target_rate,
        class_weights,
    })
}

fn evaluate_model(
    network: &mut CompetitiveSnnNetwork,
    data: &TrainingData,
    loss_config: &PsaLossConfig,
    window_size: usize,
    val_start_idx: usize,
    usable_len: usize,
) -> (f64, f64, f64) {
    let mut val_loss = 0.0;
    let mut val_samples = 0;
    let mut val_cm = ConfusionMatrix::new();
    let mut val_start = val_start_idx;

    while val_start + window_size < usable_len {
        let val_end = val_start + window_size;
        let label_idx = val_end.min(data.labels.len() - 1);
        let target = data.labels[label_idx];

        let pos_w = &data.pos_inputs[val_start..val_end];
        let neg_w = &data.neg_inputs[val_start..val_end];

        let (logits, cache) = network.forward(pos_w, neg_w);

        let window_atr = if val_end < data.atrs.len() {
            data.atrs[val_end]
        } else {
            1.0
        };
        let volatility_ratio = (window_atr / data.global_mean_atr)
            .max(0.001)
            .clamp(0.5, 2.0);

        let loss_result = rustrade::domain::snn::loss::psa_loss(
            &logits,
            target,
            cache.overall_spike_rate,
            volatility_ratio,
            loss_config,
        );

        val_loss += loss_result.total_loss;
        val_samples += 1;

        let predicted = logits
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(2);
        val_cm.record(target, predicted);

        val_start += window_size;
    }

    let avg_loss = if val_samples > 0 {
        val_loss / val_samples as f64
    } else {
        f64::NAN
    };

    (avg_loss, val_cm.macro_f1(), val_cm.mcc())
}

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    dotenvy::dotenv().ok();
    let args = Args::parse();

    tracing::info!(
        "🧠 Starting Surrogate Gradient SNN Training for {}...",
        args.symbol
    );

    let hyperparams = if let Some(hp_path) = &args.hyperparams {
        let json = std::fs::read_to_string(hp_path)?;
        serde_json::from_str::<SnnHyperparameters>(&json)?
    } else {
        SnnHyperparameters {
            learning_rate: args.lr,
            surrogate_beta: args.beta_start,
            threshold: args.threshold,
            lambda_rate: args.lambda_rate,
            lambda_silence: args.lambda_silence,
            lambda_overtrade: args.lambda_overtrade,
            ..Default::default()
        }
    };

    let data = fetch_and_encode_data(&args).await?;

    let usable_len = data.pos_inputs.len().min(data.labels.len());
    if usable_len < 20 {
        anyhow::bail!("Not enough aligned data: {} samples", usable_len);
    }

    let train_size = (usable_len as f64 * args.train_ratio) as usize;
    let val_size = usable_len - train_size;
    tracing::info!(
        "Train: {} samples, Validation: {} samples",
        train_size,
        val_size
    );

    let input_dim = if data.pos_inputs.is_empty() {
        3
    } else {
        data.pos_inputs[0].len()
    };

    let mut network = if let Some(resume_path) = &args.resume {
        let model_json = std::fs::read_to_string(resume_path)
            .map_err(|e| anyhow::anyhow!("Failed to read resume file: {}", e))?;
        tracing::info!(
            "♻️ Resuming training from pre-trained model: {}",
            resume_path
        );
        serde_json::from_str(&model_json)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize model: {}", e))?
    } else {
        CompetitiveSnnNetwork::new(
            input_dim,
            args.hidden_a,
            args.hidden_b,
            3,
            hyperparams.clone(),
        )
    };

    let surrogate_type_enum = match args.surrogate_type.to_lowercase().as_str() {
        "fast_sigmoid" | "fastsigmoid" => SurrogateType::FastSigmoid,
        "atan" => SurrogateType::ATan,
        "triangle" => SurrogateType::Triangle,
        "exponential" => SurrogateType::Exponential,
        _ => {
            tracing::warn!(
                "Unknown surrogate type '{}', defaulting to Triangle",
                args.surrogate_type
            );
            SurrogateType::Triangle
        }
    };
    network.layer_a.surrogate_type = surrogate_type_enum;
    network.layer_b.surrogate_type = surrogate_type_enum;

    let loss_config = PsaLossConfig {
        lambda_rate: args.lambda_rate,
        lambda_silence: args.lambda_silence,
        lambda_overtrade: args.lambda_overtrade,
        target_rate: data.target_rate,
        class_weights: data.class_weights.clone(),
        ..Default::default()
    };

    let mut optimizer = AdamOptimizer::new(
        args.lr,
        &[
            (input_dim, args.hidden_a),
            (input_dim, args.hidden_b),
            (args.hidden_a + args.hidden_b, 3),
            (args.hidden_a, args.hidden_b),
            (args.hidden_b, args.hidden_a),
        ],
        &[3],
    );

    let window_size = hyperparams.time_steps.min(50);
    tracing::info!("🔍 Evaluating baseline performance...");
    let (base_loss, initial_f1, base_mcc) = evaluate_model(
        &mut network,
        &data,
        &loss_config,
        window_size,
        train_size,
        usable_len,
    );
    let mut best_val_loss = 1.0 - initial_f1;
    let mut patience_counter = 0;

    tracing::info!(
        "📊 Baseline | Val Loss: {:.4} | Val F1: {:.3} | MCC: {:.3}",
        base_loss,
        initial_f1,
        base_mcc
    );

    for epoch in 1..=args.epochs {
        // P2.6: Learning rate warmup & cosine annealing
        // Linear warmup for first 10% of epochs, then cosine decay
        let warmup_epochs = (args.epochs as f64 * 0.1).max(1.0);
        let lr_t = if (epoch as f64) <= warmup_epochs {
            args.lr_min + (args.lr - args.lr_min) * (epoch as f64 / warmup_epochs)
        } else {
            let progress = (epoch as f64 - warmup_epochs) / (args.epochs as f64 - warmup_epochs);
            args.lr_min
                + 0.5 * (args.lr - args.lr_min) * (1.0 + (std::f64::consts::PI * progress).cos())
        };
        optimizer.lr = lr_t;

        let progress = (epoch - 1) as f64 / args.epochs as f64;
        let cosine_progress = 0.5 * (1.0 - (std::f64::consts::PI * progress).cos());
        let current_beta = args.beta_start + (args.beta_end - args.beta_start) * cosine_progress;

        network.layer_a.surrogate_beta = current_beta;
        network.layer_b.surrogate_beta = current_beta;

        let mut epoch_loss = 0.0;
        let mut epoch_samples = 0;
        let mut train_cm = ConfusionMatrix::new();

        let step_size = window_size / 2;
        let mut all_starts = Vec::new();
        let mut w = 0;
        while w + window_size < train_size {
            all_starts.push(w);
            w += step_size;
        }

        for batch_starts in all_starts.chunks(args.batch_size) {
            let batch_results: Vec<_> = batch_starts
                .par_iter()
                .map(|&start| {
                    let end = start + window_size;
                    let label_idx = end.min(data.labels.len() - 1);
                    let target = data.labels[label_idx];

                    let pos_window = &data.pos_inputs[start..end];
                    let neg_window = &data.neg_inputs[start..end];

                    let (logits, cache) = network.forward(pos_window, neg_window);

                    let window_atr = if end < data.atrs.len() {
                        data.atrs[end]
                    } else {
                        1.0
                    };
                    let volatility_ratio = (window_atr / data.global_mean_atr)
                        .max(0.001)
                        .clamp(0.5, 2.0);

                    let grads: SnnGradients =
                        network.backward(&cache, target, volatility_ratio, &loss_config);

                    let predicted = logits
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(i, _)| i)
                        .unwrap_or(2);

                    (grads, target, predicted)
                })
                .collect();

            let mut sum_grad_a = Array2::zeros(network.layer_a.weights.raw_dim());
            let mut sum_grad_b = Array2::zeros(network.layer_b.weights.raw_dim());
            let mut sum_grad_r = Array2::zeros(network.readout.raw_dim());
            let mut sum_grad_ab = Array2::zeros(network.inhibit_a_to_b.raw_dim());
            let mut sum_grad_ba = Array2::zeros(network.inhibit_b_to_a.raw_dim());
            let mut sum_grad_bias = Array1::zeros(network.readout_bias.raw_dim());
            let batch_len = batch_results.len() as f64;

            for (grads, target, pred) in batch_results {
                sum_grad_a += &grads.grad_layer_a;
                sum_grad_b += &grads.grad_layer_b;
                sum_grad_r += &grads.grad_readout;
                sum_grad_bias += &grads.grad_readout_bias;
                sum_grad_ab += &grads.grad_inhibit_ab;
                sum_grad_ba += &grads.grad_inhibit_ba;

                epoch_loss += grads.loss.total_loss;
                epoch_samples += 1;
                train_cm.record(target, pred);
            }

            sum_grad_a /= batch_len;
            sum_grad_b /= batch_len;
            sum_grad_r /= batch_len;
            sum_grad_ab /= batch_len;
            sum_grad_ba /= batch_len;
            sum_grad_bias /= batch_len;

            optimizer.step(
                &mut [
                    &mut network.layer_a.weights,
                    &mut network.layer_b.weights,
                    &mut network.readout,
                    &mut network.inhibit_a_to_b,
                    &mut network.inhibit_b_to_a,
                ],
                &[sum_grad_a, sum_grad_b, sum_grad_r, sum_grad_ab, sum_grad_ba],
                &mut [&mut network.readout_bias],
                &[sum_grad_bias],
            );
            network.clamp_inhibitory();

            if args.weight_decay > 0.0 {
                let decay = 1.0 - (args.weight_decay * optimizer.lr);
                // P2.7 Clarification: Decoupled L2 weight decay is applied to standard weights.
                // Inhibitory weights (inhibit_a_to_b, inhibit_b_to_a) are clamped to <= 0
                // in `clamp_inhibitory` and are NOT decayed here because they represent hard
                // biological constraints rather than standard learnable parameters.
                network.layer_a.weights.mapv_inplace(|w| w * decay);
                network.layer_b.weights.mapv_inplace(|w| w * decay);
                network.readout.mapv_inplace(|w| w * decay);
            }
        }

        if epoch_samples == 0 {
            continue;
        }

        let avg_loss = epoch_loss / epoch_samples as f64;
        let train_f1 = train_cm.macro_f1();

        let (avg_val_loss, val_f1, val_mcc) = evaluate_model(
            &mut network,
            &data,
            &loss_config,
            window_size,
            train_size,
            usable_len,
        );

        tracing::info!(
            "Epoch {:3}: Loss={:.4} F1={:.3} | Val Loss={:.4} F1={:.3} MCC={:.3} | LR={:.6}",
            epoch,
            avg_loss,
            train_f1,
            avg_val_loss,
            val_f1,
            val_mcc,
            optimizer.lr
        );

        let stop_metric = 1.0 - val_f1;

        if epoch >= args.grace_period {
            if stop_metric <= best_val_loss {
                best_val_loss = stop_metric;
                patience_counter = 0;

                let model_json = serde_json::to_string_pretty(&network)?;
                std::fs::write(&args.output, &model_json)?;
            } else {
                patience_counter += 1;
                if patience_counter >= args.patience {
                    tracing::info!(
                        "🛑 Early stopping after {} epochs without improvement",
                        args.patience
                    );
                    break;
                }
            }
        }
    }

    tracing::info!(
        "Training complete. Best validation F1 was {:.3} Checkpoint: {}",
        1.0 - best_val_loss,
        args.output
    );
    Ok(())
}
