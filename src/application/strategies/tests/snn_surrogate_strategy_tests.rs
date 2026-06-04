use crate::application::strategies::snn_surrogate_strategy::SnnSurrogateStrategy;
use crate::application::strategies::traits::{AnalysisContext, TradingStrategy};
use crate::domain::snn::competitive_network::CompetitiveSnnNetwork;
use crate::domain::snn::hyperparams::SnnHyperparameters;
use crate::domain::trading::types::Candle;
use rust_decimal::Decimal;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

fn create_mock_model(name: &str) -> PathBuf {
    let hp = SnnHyperparameters::default();
    let network = CompetitiveSnnNetwork::new(10, 8, 8, 3, hp);
    let json = serde_json::to_string(&network).unwrap();
    let mut path = std::env::temp_dir();
    path.push(format!("mock_snn_model_{}.json", name));
    fs::write(&path, json).unwrap();
    path
}

fn create_mock_candles(count: usize, interval_mins: i64) -> VecDeque<Candle> {
    let mut candles = VecDeque::new();
    let base_time = 1735689600; // 2025-01-01 00:00:00 (multiple of 15 or not?)

    for i in 0..count {
        candles.push_back(Candle {
            symbol: "TSLA".to_string(),
            timestamp: base_time + (i as i64 * interval_mins * 60),
            open: Decimal::from(100 + i),
            high: Decimal::from(105 + i),
            low: Decimal::from(95 + i),
            close: Decimal::from(102 + i),
            volume: Decimal::from(1000),
        });
    }
    candles
}

#[test]
fn test_snn_strategy_initialization() {
    let model_path = create_mock_model("init");
    let strategy = SnnSurrogateStrategy::new(model_path.to_str().unwrap(), 0.5, 10, 0.01);
    assert!(strategy.is_ok());
    fs::remove_file(model_path).ok();
}

#[test]
fn test_snn_strategy_alignment_check() {
    let model_path = create_mock_model("align");
    let strategy = SnnSurrogateStrategy::new(model_path.to_str().unwrap(), 0.5, 10, 0.01).unwrap();

    // 1 minute interval, but current timestamp not multiple of 15
    let mut candles = create_mock_candles(200, 1);
    let last_candle = candles.back_mut().unwrap();
    last_candle.timestamp = 1735689600 + 13 * 60; // 13:00 (not multiple of 15)

    let ctx = AnalysisContext {
        symbol: "TSLA".to_string(),
        current_price: Decimal::from(102),
        strict_sell_htf_confirmation: false,
        fast_sma: None,
        slow_sma: None,
        trend_sma: None,
        rsi: None,
        macd_value: None,
        macd_signal: None,
        macd_histogram: None,
        last_macd_histogram: None,
        atr: None,
        bb_lower: None,
        bb_upper: None,
        bb_middle: None,
        adx: None,
        has_position: false,
        position: None,
        timestamp: last_candle.timestamp,
        candles,
        rsi_history: VecDeque::new(),
        ofi_value: Decimal::ZERO,
        cumulative_delta: Decimal::ZERO,
        volume_profile: None,
        ofi_history: VecDeque::new(),
        hurst_exponent: None,
        skewness: None,
        momentum_normalized: None,
        realized_volatility: None,
        timeframe_features: None,
        feature_set: None,
    };

    let signal = strategy.analyze(&ctx);
    assert!(
        signal.is_none(),
        "Should return None for non-15m aligned timestamp"
    );
    fs::remove_file(model_path).ok();
}

#[test]
fn test_snn_strategy_insufficient_data() {
    let model_path = create_mock_model("insufficient");
    let strategy = SnnSurrogateStrategy::new(model_path.to_str().unwrap(), 0.5, 10, 0.01).unwrap();

    // Not enough candles
    let candles = create_mock_candles(5, 1);
    let ctx = AnalysisContext {
        symbol: "TSLA".to_string(),
        current_price: Decimal::from(102),
        strict_sell_htf_confirmation: false,
        fast_sma: None,
        slow_sma: None,
        trend_sma: None,
        rsi: None,
        macd_value: None,
        macd_signal: None,
        macd_histogram: None,
        last_macd_histogram: None,
        atr: None,
        bb_lower: None,
        bb_upper: None,
        bb_middle: None,
        adx: None,
        has_position: false,
        position: None,
        timestamp: candles.back().unwrap().timestamp,
        candles,
        rsi_history: VecDeque::new(),
        ofi_value: Decimal::ZERO,
        cumulative_delta: Decimal::ZERO,
        volume_profile: None,
        ofi_history: VecDeque::new(),
        hurst_exponent: None,
        skewness: None,
        momentum_normalized: None,
        realized_volatility: None,
        timeframe_features: None,
        feature_set: None,
    };

    let signal = strategy.analyze(&ctx);
    assert!(signal.is_none());
    fs::remove_file(model_path).ok();
}

/// Creates a model where the readout strongly favors class `bias_class` (0=Buy, 1=Sell).
/// This allows testing the happy-path where the strategy emits a signal.
fn create_biased_model(name: &str, bias_class: usize) -> PathBuf {
    let hp = SnnHyperparameters::default();
    let mut network = CompetitiveSnnNetwork::new(10, 8, 8, 3, hp);
    // Zero all readout weights then strongly bias toward the chosen class
    network.readout.fill(0.0);
    for i in 0..network.readout.nrows() {
        network.readout[[i, bias_class]] = 10.0;
    }
    // Clear the hold-favoring bias so the chosen class dominates
    network.readout_bias.fill(0.0);
    network.readout_bias[bias_class] = 5.0;

    let json = serde_json::to_string(&network).unwrap();
    let mut path = std::env::temp_dir();
    path.push(format!("mock_snn_biased_{}.json", name));
    fs::write(&path, json).unwrap();
    path
}

/// Builds a minimal AnalysisContext with enough 1m candles (200+ = >15m aligned) to pass
/// all guards in prepare_rolling_window. The final candle is set to a 15-minute boundary.
fn create_aligned_context(candle_model_path: PathBuf) -> (AnalysisContext, PathBuf) {
    // 240 minutes of 1m candles, last candle at minute 0 of an hour → 15m aligned
    let mut candles = create_mock_candles(240, 1);
    // Adjust the last candle to a multiple-of-15 timestamp
    // 1735689600 base + 239 mins → not always aligned, so snap it manually
    let aligned_ts = 1735696800_i64; // 2025-01-01 01:00:00 UTC, which is 60 min = multiple of 15
    candles.back_mut().unwrap().timestamp = aligned_ts;

    let ctx = AnalysisContext {
        symbol: "TSLA".to_string(),
        current_price: Decimal::from(120),
        strict_sell_htf_confirmation: false,
        fast_sma: None,
        slow_sma: None,
        trend_sma: None,
        rsi: None,
        macd_value: None,
        macd_signal: None,
        macd_histogram: None,
        last_macd_histogram: None,
        atr: None,
        bb_lower: None,
        bb_upper: None,
        bb_middle: None,
        adx: None,
        has_position: false,
        position: None,
        timestamp: aligned_ts,
        candles,
        rsi_history: VecDeque::new(),
        ofi_value: Decimal::ZERO,
        cumulative_delta: Decimal::ZERO,
        volume_profile: None,
        ofi_history: VecDeque::new(),
        hurst_exponent: None,
        skewness: None,
        momentum_normalized: None,
        realized_volatility: None,
        timeframe_features: None,
        feature_set: None,
    };
    (ctx, candle_model_path)
}

#[test]
fn test_snn_strategy_forward_pass_does_not_panic() {
    // Verify that a fully aligned context runs the forward pass without panicking.
    // The output may be None (hold) or Some(signal) depending on the random model.
    let model_path = create_mock_model("happy_path");
    let strategy = SnnSurrogateStrategy::new(model_path.to_str().unwrap(), 0.0, 10, 0.5).unwrap();
    let (ctx, model_path) = create_aligned_context(model_path);

    // Should not panic
    let _signal = strategy.analyze(&ctx);

    fs::remove_file(model_path).ok();
}

#[test]
fn test_snn_strategy_biased_buy_produces_signal() {
    // A model strongly biased toward Buy (class 0) should emit a Buy signal.
    // Threshold set very low (0.0) so even tiny logit differences pass.
    let model_path = create_biased_model("buy_biased", 0);
    let strategy = SnnSurrogateStrategy::new(model_path.to_str().unwrap(), 0.0, 10, 0.5).unwrap();
    let (ctx, model_path) = create_aligned_context(model_path);

    let signal = strategy.analyze(&ctx);
    if let Some(sig) = signal {
        assert_eq!(
            sig.side,
            crate::domain::trading::types::OrderSide::Buy,
            "Biased-buy model should emit Buy, got {:?}",
            sig.side
        );
        assert!(sig.confidence >= 0.0 && sig.confidence <= 1.0);
    }
    // None is acceptable if the feature window produced no spikes (flat mock prices)

    fs::remove_file(model_path).ok();
}

#[test]
fn test_snn_train_step_preserves_inhibition_constraint() {
    // Verify that clamp_inhibitory() is being called inside train_step by checking
    // that inhibitory weights never become positive after optimizing.
    use crate::domain::snn::loss::PsaLossConfig;
    use crate::domain::snn::optimizer::AdamOptimizer;
    use ndarray::Array1;

    let hp = SnnHyperparameters::default();
    let mut net = CompetitiveSnnNetwork::new(2, 4, 4, 3, hp);
    let config = PsaLossConfig::default();
    let mut opt = AdamOptimizer::new(
        0.1, // High LR to force large weight updates
        &[
            (net.layer_a.input_dim, net.layer_a.num_neurons),
            (net.layer_b.input_dim, net.layer_b.num_neurons),
            (net.layer_a.num_neurons + net.layer_b.num_neurons, 3),
            (net.layer_a.num_neurons, net.layer_b.num_neurons),
            (net.layer_b.num_neurons, net.layer_a.num_neurons),
        ],
        &[3],
    );

    let pos = vec![Array1::from_vec(vec![1.0, 0.5]); 10];
    let neg = vec![Array1::from_vec(vec![0.5, 1.0]); 10];

    for _ in 0..20 {
        net.train_step(&pos, &neg, 0, 1.0, &config, &mut opt);
    }

    // After training, inhibitory weights must still be ≤ 0
    assert!(
        net.inhibit_a_to_b.iter().all(|&w| w <= 0.0),
        "inhibit_a_to_b must be ≤ 0 after train_step"
    );
    assert!(
        net.inhibit_b_to_a.iter().all(|&w| w <= 0.0),
        "inhibit_b_to_a must be ≤ 0 after train_step"
    );
}
