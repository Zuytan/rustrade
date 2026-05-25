//! Penalized Spike Accuracy (PSA) Loss Function for Trading SNN.
//!
//! This custom loss function addresses the unique challenges of training
//! a Spiking Neural Network for high-frequency trading:
//!
//! 1. **Classification**: Correct buy/sell/hold prediction (cross-entropy).
//! 2. **Spike rate regulation**: Prevents the network from over-trading or going silent.
//! 3. **Silence penalty**: Extra cost for completely dead networks (common failure mode in SNN training).
//!
//! The loss is fully differentiable and returns analytical gradients.

use ndarray::Array1;

/// Configuration for the PSA loss function.
///
/// All `lambda_*` parameters are regularization weights that control
/// the relative importance of each penalty term. These are suitable
/// for tuning via Bayesian optimization.
#[derive(Debug, Clone)]
pub struct PsaLossConfig {
    /// Weight for spike rate deviation penalty. Typical: 0.1.
    pub lambda_rate: f64,
    /// Weight for silence penalty. Typical: 1.0 (aggressive anti-silence).
    pub lambda_silence: f64,
    /// Weight for over-trading penalty. Typical: 0.5.
    pub lambda_overtrade: f64,
    /// Target spike rate (fraction of timesteps with output spikes).
    /// Should match the empirical frequency of significant market events.
    /// E.g., if 5% of bars have meaningful moves: target_rate = 0.05.
    pub target_rate: f64,
    /// Spike rate below this threshold triggers the silence penalty.
    pub silence_threshold: f64,
    /// Spike rate above `target_rate * overtrade_factor` triggers over-trading penalty.
    pub overtrade_factor: f64,
    /// Number of output classes (default: 3 for buy/sell/hold).
    pub num_classes: usize,
    /// Small constant to avoid log(0) in cross-entropy.
    pub epsilon: f64,
    /// Optional class weights for cross-entropy (inverse frequency).
    /// If present, CE = -w[c] * log(p[c]).
    pub class_weights: Option<Array1<f64>>,
}

impl Default for PsaLossConfig {
    fn default() -> Self {
        Self {
            lambda_rate: 0.1,
            lambda_silence: 1.0,
            lambda_overtrade: 0.5,
            target_rate: 0.05,
            silence_threshold: 0.01,
            overtrade_factor: 3.0,
            num_classes: 3,
            epsilon: 1e-7,
            class_weights: None,
        }
    }
}

/// Result of the PSA loss computation.
#[derive(Debug, Clone)]
pub struct PsaLossResult {
    /// Total loss value (CE + penalties).
    pub total_loss: f64,
    /// Cross-entropy component.
    pub ce_loss: f64,
    /// Spike rate deviation penalty.
    pub rate_penalty: f64,
    /// Silence penalty.
    pub silence_penalty: f64,
    /// Over-trading penalty.
    pub overtrade_penalty: f64,
    /// Gradient of total loss w.r.t. predictions [num_classes].
    pub grad_predictions: Array1<f64>,
    /// Gradient contribution to spike rate (scalar, to propagate through spike generation).
    pub grad_spike_rate: f64,
}

/// Computes the softmax of a vector (numerically stable).
///
/// Uses the log-sum-exp trick: subtract max before exponentiating
/// to prevent overflow with large logits.
fn softmax(logits: &Array1<f64>) -> Array1<f64> {
    let max_val = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exp_logits = logits.mapv(|x| (x - max_val).exp());
    let sum = exp_logits.sum();
    exp_logits / sum
}

/// Computes the PSA loss and its gradients.
///
/// # Arguments
/// * `predictions` - Raw logits (readout values) [num_classes]. NOT softmaxed.
/// * `target_class` - Ground truth class index (0=buy, 1=sell, 2=hold).
/// * `spike_rate` - Observed network spike rate (mean spikes per timestep, in [0, 1]).
/// * `config` - Loss function configuration.
///
/// # Returns
/// `PsaLossResult` containing the loss value, its components, and gradients.
///
/// # Loss Formula
/// ```text
/// L = CE(softmax(pred), target) + λ_rate * (rate - target_rate)²
///     + λ_silence * max(0, θ_s - rate)² + λ_overtrade * max(0, rate - k * target_rate)²
/// ```
pub fn psa_loss(
    predictions: &Array1<f64>,
    target_class: usize,
    spike_rate: f64,
    volatility_ratio: f64,
    config: &PsaLossConfig,
) -> PsaLossResult {
    // 1. Cross-entropy loss with softmax
    let probs = softmax(predictions);

    // CE = -log(p[target_class])
    let weight = config
        .class_weights
        .as_ref()
        .map_or(1.0, |w| w[target_class]);
    let ce_loss = -weight * (probs[target_class] + config.epsilon).ln();

    // Gradient of CE w.r.t. logits (predictions):
    // ∂CE/∂z_i = w[target] * (p_i - 1_{i == target})
    let mut grad_ce = probs.clone();
    for (i, p) in grad_ce.iter_mut().enumerate() {
        if i == target_class {
            *p = weight * (*p - 1.0);
        } else {
            *p *= weight;
        }
    }

    // 2. Spike rate deviation penalty: λ_rate * (rate - target_rate)²
    let rate_diff = spike_rate - config.target_rate;
    let rate_penalty = config.lambda_rate * rate_diff * rate_diff;
    let grad_rate_from_deviation = 2.0 * config.lambda_rate * rate_diff;

    // Apply dynamic risk penalty clamping
    let v_ratio = volatility_ratio.clamp(0.5, 2.0);
    let dynamic_lambda_silence = config.lambda_silence / v_ratio;
    let dynamic_lambda_overtrade = config.lambda_overtrade * v_ratio;

    // 3. Silence penalty: λ_silence * max(0, θ_s - rate)²
    let silence_deficit = (config.silence_threshold - spike_rate).max(0.0);
    let silence_penalty = dynamic_lambda_silence * silence_deficit * silence_deficit;
    let grad_rate_from_silence = if spike_rate < config.silence_threshold {
        -2.0 * dynamic_lambda_silence * silence_deficit
    } else {
        0.0
    };

    // 4. Over-trading penalty: λ_overtrade * max(0, rate - k * target_rate)²
    let overtrade_threshold = config.target_rate * config.overtrade_factor;
    let overtrade_excess = (spike_rate - overtrade_threshold).max(0.0);
    let overtrade_penalty = dynamic_lambda_overtrade * overtrade_excess * overtrade_excess;
    let grad_rate_from_overtrade = if spike_rate > overtrade_threshold {
        2.0 * dynamic_lambda_overtrade * overtrade_excess
    } else {
        0.0
    };

    // Total
    let total_loss = ce_loss + rate_penalty + silence_penalty + overtrade_penalty;
    let grad_spike_rate =
        grad_rate_from_deviation + grad_rate_from_silence + grad_rate_from_overtrade;

    PsaLossResult {
        total_loss,
        ce_loss,
        rate_penalty,
        silence_penalty,
        overtrade_penalty,
        grad_predictions: grad_ce,
        grad_spike_rate,
    }
}

/// Computes target labels from price returns.
///
/// # Arguments
/// * `returns` - Sequence of price returns (e.g., `(p[t+5] - p[t]) / p[t]`).
/// * `threshold` - Minimum |return| to classify as buy/sell.
///
/// # Returns
/// Vector of class labels: 0 = buy (return > threshold), 1 = sell (return < -threshold), 2 = hold.
pub fn returns_to_labels(returns: &[f64], threshold: f64) -> Vec<usize> {
    returns
        .iter()
        .map(|&r| {
            if r >= threshold {
                0 // Buy
            } else if r <= -threshold {
                1 // Sell
            } else {
                2 // Hold
            }
        })
        .collect()
}

/// Computes the target spike rate from labels.
///
/// The target rate is the fraction of labels that are NOT hold (i.e., buy or sell).
/// This gives the network a baseline for how active it should be.
pub fn compute_target_rate(labels: &[usize]) -> f64 {
    if labels.is_empty() {
        return 0.05; // Safe default
    }
    let active = labels.iter().filter(|&&l| l != 2).count();
    active as f64 / labels.len() as f64
}

/// Computes inverse-frequency class weights from labels.
///
/// w[c] = N / (num_classes * count[c])
pub fn compute_class_weights(labels: &[usize], num_classes: usize) -> Array1<f64> {
    let mut counts = vec![0usize; num_classes];
    for &l in labels {
        if l < num_classes {
            counts[l] += 1;
        }
    }

    let total = labels.len() as f64;
    let mut weights = Array1::zeros(num_classes);
    for i in 0..num_classes {
        // Smoothing to avoid division by zero or extreme weights
        let count = counts[i].max(1) as f64;
        weights[i] = total / (num_classes as f64 * count);
    }

    // Normalize so mean weight is 1.0
    let mean = weights.mean().expect("class weights cannot be empty");
    weights / mean
}

/// A 3x3 Confusion Matrix for buy/sell/hold diagnostics.
#[derive(Debug, Clone, Default)]
pub struct ConfusionMatrix {
    /// matrix[actual][predicted]
    pub matrix: [[usize; 3]; 3],
}

impl ConfusionMatrix {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, actual: usize, predicted: usize) {
        if actual < 3 && predicted < 3 {
            self.matrix[actual][predicted] += 1;
        }
    }

    /// Macro-averaged F1 score (average of F1 for each class).
    /// High value means the model performs well on all classes, including minority.
    pub fn macro_f1(&self) -> f64 {
        let mut sum_f1 = 0.0;
        for i in 0..3 {
            let tp = self.matrix[i][i] as f64;
            let fp = (0..3)
                .filter(|&j| j != i)
                .map(|j| self.matrix[j][i])
                .sum::<usize>() as f64;
            let fn_ = (0..3)
                .filter(|&j| j != i)
                .map(|j| self.matrix[i][j])
                .sum::<usize>() as f64;

            let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };
            let recall = if tp + fn_ > 0.0 { tp / (tp + fn_) } else { 0.0 };
            let correct_f1 = if precision + recall > 0.0 {
                2.0 * (precision * recall) / (precision + recall)
            } else {
                0.0
            };
            sum_f1 += correct_f1;
        }
        sum_f1 / 3.0
    }

    /// Matthews Correlation Coefficient for multiclass.
    /// Range [-1, 1], where 1 is perfect, 0 is random, and -1 is total disagreement.
    pub fn mcc(&self) -> f64 {
        let mut n = 0.0;
        let mut c = 0.0;
        let mut t = [0.0; 3];
        let mut p = [0.0; 3];

        for (i, t_i) in t.iter_mut().enumerate() {
            for (j, p_j) in p.iter_mut().enumerate() {
                let count = self.matrix[i][j] as f64;
                n += count;
                if i == j {
                    c += count;
                }
                *t_i += count; // actual i total
                *p_j += count; // predicted j total
            }
        }

        if n == 0.0 {
            return 0.0;
        }

        // Multiclass MCC formula (Gorodkin 2004)
        let numerator = c * n - (0..3).map(|i| t[i] * p[i]).sum::<f64>();
        let denom_actual = n * n - (0..3).map(|i| t[i] * t[i]).sum::<f64>();
        let denom_pred = n * n - (0..3).map(|i| p[i] * p[i]).sum::<f64>();

        if denom_actual <= 0.0 || denom_pred <= 0.0 {
            return 0.0;
        }

        numerator / (denom_actual * denom_pred).sqrt()
    }

    pub fn display(&self) -> String {
        format!(
            "Confusion Matrix:\n\
             \tPred: B  S  H\n\
             Act B: {:2} {:2} {:2}\n\
             Act S: {:2} {:2} {:2}\n\
             Act H: {:2} {:2} {:2}",
            self.matrix[0][0],
            self.matrix[0][1],
            self.matrix[0][2],
            self.matrix[1][0],
            self.matrix[1][1],
            self.matrix[1][2],
            self.matrix[2][0],
            self.matrix[2][1],
            self.matrix[2][2],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_softmax_sums_to_one() {
        let logits = Array1::from_vec(vec![1.0, 2.0, 3.0]);
        let probs = softmax(&logits);
        assert!(
            (probs.sum() - 1.0).abs() < 1e-10,
            "Softmax should sum to 1.0, got {}",
            probs.sum()
        );
    }

    #[test]
    fn test_softmax_largest_input_gets_highest_prob() {
        let logits = Array1::from_vec(vec![1.0, 5.0, 2.0]);
        let probs = softmax(&logits);
        assert!(probs[1] > probs[0] && probs[1] > probs[2]);
    }

    #[test]
    fn test_softmax_numerical_stability() {
        // Very large values should not overflow
        let logits = Array1::from_vec(vec![1000.0, 1001.0, 999.0]);
        let probs = softmax(&logits);
        assert!(probs.iter().all(|p| p.is_finite()), "Softmax overflowed");
        assert!((probs.sum() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_psa_loss_perfect_prediction() {
        let config = PsaLossConfig::default();
        // Logits strongly favor class 0 (buy)
        let predictions = Array1::from_vec(vec![10.0, -10.0, -10.0]);
        let result = psa_loss(&predictions, 0, config.target_rate, 1.0, &config);

        // CE should be very small for correct prediction
        assert!(
            result.ce_loss < 0.001,
            "CE loss should be near zero for correct prediction, got {}",
            result.ce_loss
        );
    }

    #[test]
    fn test_psa_loss_wrong_prediction() {
        let config = PsaLossConfig::default();
        // Logits strongly favor class 0, but target is class 1
        let predictions = Array1::from_vec(vec![10.0, -10.0, -10.0]);
        let result = psa_loss(&predictions, 1, config.target_rate, 1.0, &config);

        // CE should be large
        assert!(
            result.ce_loss > 5.0,
            "CE loss should be large for wrong prediction, got {}",
            result.ce_loss
        );
    }

    #[test]
    fn test_psa_rate_penalty_at_target() {
        let config = PsaLossConfig::default();
        let predictions = Array1::from_vec(vec![0.0, 0.0, 0.0]);
        let result = psa_loss(&predictions, 2, config.target_rate, 1.0, &config);

        assert!(
            result.rate_penalty < 1e-15,
            "Rate penalty should be zero when rate == target, got {}",
            result.rate_penalty
        );
    }

    #[test]
    fn test_psa_rate_penalty_deviation() {
        let config = PsaLossConfig::default();
        let predictions = Array1::from_vec(vec![0.0, 0.0, 0.0]);

        // Spike rate much higher than target
        let result = psa_loss(&predictions, 2, 0.5, 1.0, &config);
        assert!(
            result.rate_penalty > 0.0,
            "Rate penalty should be positive when rate deviates"
        );
    }

    #[test]
    fn test_psa_silence_penalty_activates() {
        let config = PsaLossConfig::default();
        let predictions = Array1::from_vec(vec![0.0, 0.0, 0.0]);

        // Zero spike rate → silence penalty
        let result = psa_loss(&predictions, 2, 0.0, 1.0, &config);
        assert!(
            result.silence_penalty > 0.0,
            "Silence penalty should activate at zero spike rate"
        );
    }

    #[test]
    fn test_psa_silence_penalty_inactive_above_threshold() {
        let config = PsaLossConfig::default();
        let predictions = Array1::from_vec(vec![0.0, 0.0, 0.0]);

        // Spike rate above silence threshold
        let result = psa_loss(&predictions, 2, 0.05, 1.0, &config);
        assert_eq!(
            result.silence_penalty, 0.0,
            "Silence penalty should be zero above threshold"
        );
    }

    #[test]
    fn test_psa_overtrade_penalty() {
        let config = PsaLossConfig {
            target_rate: 0.05,
            overtrade_factor: 2.0,
            ..PsaLossConfig::default()
        };
        let predictions = Array1::from_vec(vec![0.0, 0.0, 0.0]);

        // Spike rate 3x target (exceeds 2x threshold)
        let result = psa_loss(&predictions, 2, 0.15, 1.0, &config);
        assert!(
            result.overtrade_penalty > 0.0,
            "Overtrade penalty should be positive when rate > k * target"
        );
    }

    #[test]
    fn test_psa_gradient_shape() {
        let config = PsaLossConfig::default();
        let predictions = Array1::from_vec(vec![1.0, 2.0, 3.0]);
        let result = psa_loss(&predictions, 0, 0.05, 1.0, &config);
        assert_eq!(result.grad_predictions.len(), 3);
    }

    #[test]
    fn test_psa_gradient_direction() {
        let config = PsaLossConfig::default();
        // Logits favor class 2, target is class 0
        let predictions = Array1::from_vec(vec![-5.0, -5.0, 5.0]);
        let result = psa_loss(&predictions, 0, 0.05, 1.0, &config);

        // Gradient for target class should be negative (push logit up)
        assert!(
            result.grad_predictions[0] < 0.0,
            "Gradient for target class should be negative (increase logit)"
        );
        // Gradient for over-confident wrong class should be positive (push logit down)
        assert!(
            result.grad_predictions[2] > 0.0,
            "Gradient for wrong class should be positive (decrease logit)"
        );
    }

    #[test]
    fn test_returns_to_labels() {
        let returns = vec![0.01, -0.02, 0.001, -0.001, 0.005];
        let labels = returns_to_labels(&returns, 0.005);
        assert_eq!(labels, vec![0, 1, 2, 2, 0]); // buy, sell, hold, hold, buy
    }

    #[test]
    fn test_compute_target_rate() {
        let labels = vec![0, 1, 2, 2, 2, 0, 2, 1, 2, 2];
        let rate = compute_target_rate(&labels);
        // 4 active (0,1,0,1) out of 10 = 0.4
        assert!(
            (rate - 0.4).abs() < 1e-10,
            "Target rate should be 0.4, got {}",
            rate
        );
    }

    #[test]
    fn test_compute_class_weights() {
        // 90 hold (2), 5 buy (0), 5 sell (1)
        let mut labels = vec![2; 90];
        labels.extend(vec![0; 5]);
        labels.extend(vec![1; 5]);

        let weights = compute_class_weights(&labels, 3);

        // Hold should have small weight, buy/sell should have large weight
        assert!(weights[2] < weights[0]);
        assert!(weights[2] < weights[1]);
        assert!((weights[0] - weights[1]).abs() < 1e-10);

        // Mean weight should be 1.0 (normalization check)
        assert!((weights.mean().unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_psa_loss_weighted_ce() {
        let mut config = PsaLossConfig::default();
        let weights = Array1::from_vec(vec![10.0, 10.0, 0.1]); // Heavy on buy/sell
        config.class_weights = Some(weights);

        // Logits favoring class 2 (hold) when target is 0 (buy)
        let predictions = Array1::from_vec(vec![-5.0, -5.0, 5.0]);
        let result = psa_loss(&predictions, 0, 0.05, 1.0, &config);

        // Loss should be very large due to weighting
        assert!(result.ce_loss > 50.0);

        // Gradient for class 0 should be very negative
        assert!(result.grad_predictions[0] < -9.0);
    }

    #[test]
    fn test_confusion_matrix_metrics() {
        let mut cm = ConfusionMatrix::new();
        // Perfect predictions for buy/sell, some mistakes for hold
        for _ in 0..10 {
            cm.record(0, 0);
        } // Act B, Pred B
        for _ in 0..10 {
            cm.record(1, 1);
        } // Act S, Pred S
        for _ in 0..80 {
            cm.record(2, 2);
        } // Act H, Pred H

        let f1 = cm.macro_f1();
        assert!((f1 - 1.0).abs() < 1e-10);

        let mcc = cm.mcc();
        assert!((mcc - 1.0).abs() < 1e-10);

        // Random-like behavior
        let mut cm_rand = ConfusionMatrix::new();
        for i in 0..3 {
            for j in 0..3 {
                cm_rand.record(i, j); // 1 in every cell
            }
        }
        assert!(cm_rand.mcc() < 0.1);
    }

    #[test]
    fn test_confusion_matrix_display() {
        let mut cm = ConfusionMatrix::new();
        cm.record(0, 0);
        cm.record(1, 1);
        cm.record(2, 2);
        let s = cm.display();
        assert!(s.contains("Act B:  1  0  0"));
    }

    #[test]
    fn test_psa_loss_volatility_scaling() {
        // High volatility -> lower silence penalty, higher overtrade penalty
        let logits = Array1::from_vec(vec![0.5, -0.5, 0.1]); // predicts buy
        let target = 0; // buy (correct)

        let config = PsaLossConfig {
            lambda_silence: 1.0,
            lambda_overtrade: 1.0,
            ..Default::default()
        };

        // Normal volatility
        let _res_normal = psa_loss(&logits, target, 0.1, 1.0, &config);

        // High volatility
        let _res_high = psa_loss(&logits, target, 0.1, 2.0, &config);

        // High volatility should increase overtrade penalty and decrease silence penalty.
        // Because target is 0 (trade), silence penalty is 0, overtrade is 0.
        // Let's use target=2 (hold) to trigger penalties differently.

        // overtrade_threshold = 0.05 * 3.0 = 0.15. So we need spike_rate > 0.15 to trigger it.
        let target_hold = 2; // hold (should not trade)
        let res_normal_hold = psa_loss(&logits, target_hold, 0.2, 1.0, &config);
        let res_high_hold = psa_loss(&logits, target_hold, 0.2, 2.0, &config);

        // overtrade_penalty = lambda_overtrade * volatility_ratio * ...
        // So high volatility -> higher overtrade penalty -> higher total loss
        assert!(
            res_high_hold.total_loss > res_normal_hold.total_loss,
            "High volatility should increase overtrade penalty"
        );

        let logits_hold = Array1::from_vec(vec![-0.5, -0.5, 0.5]); // predicts hold
        let target_trade = 0; // target buy (should trade, but predicted hold)

        // silence_threshold = 0.01. So we need spike_rate < 0.01 to trigger it.
        let res_normal_trade = psa_loss(&logits_hold, target_trade, 0.0, 1.0, &config);
        let res_high_trade = psa_loss(&logits_hold, target_trade, 0.0, 2.0, &config);

        // Logits predict hold, but target is trade. This triggers silence penalty.
        // silence_penalty = lambda_silence / volatility_ratio * ...
        // So high volatility -> lower silence penalty -> lower total loss
        assert!(
            res_high_trade.total_loss < res_normal_trade.total_loss,
            "High volatility should decrease silence penalty"
        );
    }
}
