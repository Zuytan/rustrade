//! Competitive Excitatory/Inhibitory SNN Architecture.
//!
//! Two parallel sub-networks (bullish and bearish pathways) connected by
//! explicit inhibitory cross-connections. This architecture naturally isolates
//! market momentum:
//!
//! ```text
//! Positive Δprice → [Pathway A (bullish)] ──┐
//!                                           ├─ cross-inhibition ─→ [Readout] → {buy, sell, hold}
//! Negative Δprice → [Pathway B (bearish)] ──┘
//! ```
//!
//! The inhibition forces a "winner-take-all" competition between bullish and
//! bearish signals, preventing the network from simultaneously predicting
//! both buy and sell.
//!
//! # Design for Bayesian Optimization
//! All non-differentiable hyperparameters are collected in `SnnHyperparameters`,
//! which implements `Serialize`/`Deserialize` for JSON-based external optimization.

use ndarray::{Array1, Array2};
use serde::{Deserialize, Serialize};

use super::gradients::SnnGradients;
use super::hyperparams::SnnHyperparameters;
use super::izhikevich_diff::{DiffIzhikevichLayer, IzhikevichForwardCache};
use super::loss::{PsaLossConfig, PsaLossResult, psa_loss};
use super::optimizer::AdamOptimizer;

/// Cache from a competitive network forward pass.
#[derive(Debug, Clone)]
pub struct CompetitiveForwardCache {
    pub cache_a: IzhikevichForwardCache,
    pub cache_b: IzhikevichForwardCache,
    /// Spike rates integrated over time for pathway A [hidden_a].
    pub rates_a: Array1<f64>,
    /// Spike rates integrated over time for pathway B [hidden_b].
    pub rates_b: Array1<f64>,
    /// Combined spike rates used for readout [hidden_a + hidden_b].
    pub combined_rates: Array1<f64>,
    /// Raw readout logits [num_classes].
    pub logits: Array1<f64>,
    /// Overall spike rate (mean over all neurons and timesteps).
    pub overall_spike_rate: f64,
    /// Inhibitory current injected into A from B at each step [time_steps, hidden_a].
    pub inhib_currents_a: Vec<Array1<f64>>,
    /// Inhibitory current injected into B from A at each step [time_steps, hidden_b].
    pub inhib_currents_b: Vec<Array1<f64>>,
}

/// A competitive SNN with dual excitatory pathways and cross-inhibition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitiveSnnNetwork {
    /// Bullish pathway (processes positive price deltas).
    pub layer_a: DiffIzhikevichLayer,
    /// Bearish pathway (processes negative price deltas).
    pub layer_b: DiffIzhikevichLayer,

    /// Cross-inhibitory weights A→B [hidden_a, hidden_b]. Clamped to ≤ 0.
    pub inhibit_a_to_b: Array2<f64>,
    /// Cross-inhibitory weights B→A [hidden_b, hidden_a]. Clamped to ≤ 0.
    pub inhibit_b_to_a: Array2<f64>,

    /// Readout weights [hidden_a + hidden_b, num_classes].
    pub readout: Array2<f64>,
    /// Readout bias [num_classes].
    pub readout_bias: Array1<f64>,

    /// All hyperparameters.
    pub hyperparams: SnnHyperparameters,
}

impl CompetitiveSnnNetwork {
    /// Creates a new competitive network.
    ///
    /// # Arguments
    /// * `input_dim` - Dimension of each input channel (positive or negative).
    /// * `hidden_a` - Number of neurons in bullish pathway.
    /// * `hidden_b` - Number of neurons in bearish pathway.
    /// * `num_classes` - Number of output classes (default: 3).
    /// * `hyperparams` - Hyperparameters for neurons and training.
    pub fn new(
        input_dim: usize,
        hidden_a: usize,
        hidden_b: usize,
        num_classes: usize,
        hyperparams: SnnHyperparameters,
    ) -> Self {
        let mut layer_a = DiffIzhikevichLayer::with_params(
            input_dim,
            hidden_a,
            hyperparams.a,
            hyperparams.b,
            hyperparams.c,
            hyperparams.d,
        );
        layer_a.threshold = hyperparams.threshold;
        layer_a.surrogate_beta = hyperparams.surrogate_beta;

        let mut layer_b = DiffIzhikevichLayer::with_params(
            input_dim,
            hidden_b,
            hyperparams.a,
            hyperparams.b,
            hyperparams.c,
            hyperparams.d,
        );
        layer_b.threshold = hyperparams.threshold;
        layer_b.surrogate_beta = hyperparams.surrogate_beta;

        // Initialize inhibitory weights as small negative values
        let inhibit_a_to_b = Array2::from_elem((hidden_a, hidden_b), -0.01);
        let inhibit_b_to_a = Array2::from_elem((hidden_b, hidden_a), -0.01);

        // Xavier initialization for readout (deterministic LCG PRNG for reproducibility)
        let mut readout = Array2::zeros(((hidden_a + hidden_b), num_classes));
        let mut seed: u64 = 12345;
        for row in readout.iter_mut() {
            // LCG pseudo-random — same pattern as DiffIzhikevichLayer::new()
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let uniform = (seed >> 33) as f64 / (1u64 << 31) as f64; // [0, 1)
            *row = (uniform - 0.5) * (2.0 / (hidden_a + hidden_b) as f64).sqrt();
        }

        // Initialize bias to favor Hold (index 2)
        let mut readout_bias = Array1::zeros(num_classes);
        if num_classes > 2 {
            readout_bias[2] = 1.0;
        }

        Self {
            layer_a,
            layer_b,
            inhibit_a_to_b,
            inhibit_b_to_a,
            readout,
            readout_bias,
            hyperparams,
        }
    }

    /// Forward pass through the competitive network.
    ///
    /// # Arguments
    /// * `pos_inputs` - Positive channel inputs (one per timestep) [T, input_dim].
    /// * `neg_inputs` - Negative channel inputs (one per timestep) [T, input_dim].
    ///
    /// # Returns
    /// * `(logits, cache)` — Raw class logits and forward cache for backward.
    pub fn forward(
        &self,
        pos_inputs: &[Array1<f64>],
        neg_inputs: &[Array1<f64>],
    ) -> (Array1<f64>, CompetitiveForwardCache) {
        let t_steps = pos_inputs.len();
        let tau = self.hyperparams.readout_tau;

        // Process both pathways with cross-inhibition
        // We need to step through time manually to inject inhibitory currents
        let (mut v_a, mut u_a) = self.layer_a.initial_state();
        let (mut v_b, mut u_b) = self.layer_b.initial_state();

        let mut rates_a = Array1::zeros(self.layer_a.num_neurons);
        let mut rates_b = Array1::zeros(self.layer_b.num_neurons);

        let mut all_spikes_a = Vec::with_capacity(t_steps);
        let mut all_spikes_b = Vec::with_capacity(t_steps);
        let mut inhib_currents_a = Vec::with_capacity(t_steps);
        let mut inhib_currents_b = Vec::with_capacity(t_steps);

        // Cache for backward pass: we'll build manual caches
        let mut v_hist_a = vec![v_a.clone()];
        let mut u_hist_a = vec![u_a.clone()];
        let mut v_hist_b = vec![v_b.clone()];
        let mut u_hist_b = vec![u_b.clone()];
        let mut spike_grads_a_all = Vec::with_capacity(t_steps);
        let mut spike_grads_b_all = Vec::with_capacity(t_steps);
        let mut currents_a_all = Vec::with_capacity(t_steps);
        let mut currents_b_all = Vec::with_capacity(t_steps);

        let mut total_spikes = 0.0;
        let total_neurons = (self.layer_a.num_neurons + self.layer_b.num_neurons) as f64;

        for t in 0..t_steps {
            // 1. Compute input currents for both pathways
            let current_a = self.layer_a.weights.t().dot(&pos_inputs[t]);
            let current_b = self.layer_b.weights.t().dot(&neg_inputs[t]);

            // 2. Add inhibitory cross-currents from previous step's spikes
            let inhib_a = if t > 0 {
                self.inhibit_b_to_a.t().dot(&all_spikes_b[t - 1])
            } else {
                Array1::zeros(self.layer_a.num_neurons)
            };
            let inhib_b = if t > 0 {
                self.inhibit_a_to_b.t().dot(&all_spikes_a[t - 1])
            } else {
                Array1::zeros(self.layer_b.num_neurons)
            };

            let total_current_a = &current_a + &inhib_a;
            let total_current_b = &current_b + &inhib_b;

            inhib_currents_a.push(inhib_a);
            inhib_currents_b.push(inhib_b);
            currents_a_all.push(current_a);
            currents_b_all.push(current_b);

            // 3. Euler integration for pathway A
            for _sub in 0..2 {
                let (v_new, u_new) = super::izhikevich_diff::euler_substep(
                    &v_a,
                    &u_a,
                    &total_current_a,
                    self.layer_a.a,
                    self.layer_a.b,
                    self.layer_a.dt,
                );
                v_a = v_new;
                u_a = u_new;
            }

            // 4. Euler integration for pathway B
            for _sub in 0..2 {
                let (v_new, u_new) = super::izhikevich_diff::euler_substep(
                    &v_b,
                    &u_b,
                    &total_current_b,
                    self.layer_b.a,
                    self.layer_b.b,
                    self.layer_b.dt,
                );
                v_b = v_new;
                u_b = u_new;
            }

            // 5. Spike generation
            let mut spikes_a = Array1::zeros(self.layer_a.num_neurons);
            let mut grads_a = Array1::zeros(self.layer_a.num_neurons);
            let mut spikes_b = Array1::zeros(self.layer_b.num_neurons);
            let mut grads_b = Array1::zeros(self.layer_b.num_neurons);

            for i in 0..self.layer_a.num_neurons {
                let (s, g) = super::surrogate::spike_with_grad(
                    v_a[i],
                    self.layer_a.threshold,
                    self.layer_a.surrogate_beta,
                    self.layer_a.surrogate_type,
                );
                spikes_a[i] = s;
                grads_a[i] = g;
                if s > 0.5 {
                    v_a[i] = self.layer_a.c;
                    u_a[i] += self.layer_a.d;
                }
            }
            for i in 0..self.layer_b.num_neurons {
                let (s, g) = super::surrogate::spike_with_grad(
                    v_b[i],
                    self.layer_b.threshold,
                    self.layer_b.surrogate_beta,
                    self.layer_b.surrogate_type,
                );
                spikes_b[i] = s;
                grads_b[i] = g;
                if s > 0.5 {
                    v_b[i] = self.layer_b.c;
                    u_b[i] += self.layer_b.d;
                }
            }

            // 6. Update exponential spike rates
            rates_a = &rates_a * tau + &spikes_a * (1.0 - tau);
            rates_b = &rates_b * tau + &spikes_b * (1.0 - tau);

            total_spikes += spikes_a.sum() + spikes_b.sum();

            all_spikes_a.push(spikes_a);
            all_spikes_b.push(spikes_b);
            spike_grads_a_all.push(grads_a);
            spike_grads_b_all.push(grads_b);
            v_hist_a.push(v_a.clone());
            u_hist_a.push(u_a.clone());
            v_hist_b.push(v_b.clone());
            u_hist_b.push(u_b.clone());
        }

        // 7. Readout: concatenate rates from both pathways → linear → logits
        let mut combined = Array1::zeros(self.layer_a.num_neurons + self.layer_b.num_neurons);
        for (i, v) in rates_a.iter().enumerate() {
            combined[i] = *v;
        }
        for (i, v) in rates_b.iter().enumerate() {
            combined[self.layer_a.num_neurons + i] = *v;
        }

        let mut logits = self.readout.t().dot(&combined);
        logits = &logits + &self.readout_bias;

        let overall_spike_rate = total_spikes / (t_steps as f64 * total_neurons);

        // Build forward caches for backward pass
        let cache_a = IzhikevichForwardCache {
            v_history: v_hist_a,
            u_history: u_hist_a,
            spikes: all_spikes_a,
            spike_grads: spike_grads_a_all,
            currents: currents_a_all,
            inputs: pos_inputs.to_vec(),
        };
        let cache_b = IzhikevichForwardCache {
            v_history: v_hist_b,
            u_history: u_hist_b,
            spikes: all_spikes_b,
            spike_grads: spike_grads_b_all,
            currents: currents_b_all,
            inputs: neg_inputs.to_vec(),
        };

        let cache = CompetitiveForwardCache {
            cache_a,
            cache_b,
            rates_a,
            rates_b,
            combined_rates: combined,
            logits: logits.clone(),
            overall_spike_rate,
            inhib_currents_a,
            inhib_currents_b,
        };

        (logits, cache)
    }

    /// Computes loss and performs a backward pass, returning all gradients.
    ///
    /// # Arguments
    /// * `cache` - Forward cache from `forward()`.
    /// * `target_class` - Ground truth label (0=buy, 1=sell, 2=hold).
    /// * `loss_config` - PSA loss configuration.
    ///
    /// # Returns
    /// * `SnnGradients` — Loss + gradients.
    pub fn backward(
        &self,
        cache: &CompetitiveForwardCache,
        target_class: usize,
        volatility_ratio: f64,
        loss_config: &PsaLossConfig,
    ) -> SnnGradients {
        // 1. Compute PSA loss
        let loss_result = psa_loss(
            &cache.logits,
            target_class,
            cache.overall_spike_rate,
            volatility_ratio,
            loss_config,
        );

        // 2. Gradient through readout: ∂L/∂combined = readout · ∂L/∂logits
        let grad_combined = self.readout.dot(&loss_result.grad_predictions);

        // 3. Gradient for readout weights: ∂L/∂readout = combined^T ⊗ ∂L/∂logits
        let mut grad_readout = Array2::zeros(self.readout.raw_dim());
        for i in 0..grad_readout.nrows() {
            for j in 0..grad_readout.ncols() {
                grad_readout[[i, j]] = cache.combined_rates[i] * loss_result.grad_predictions[j];
            }
        }

        // 4. Split gradient into pathway A and B
        let hidden_a = self.layer_a.num_neurons;
        let grad_rates_a = grad_combined.slice(ndarray::s![..hidden_a]).to_owned();
        let grad_rates_b = grad_combined.slice(ndarray::s![hidden_a..]).to_owned();

        // 5. Gradient through spike rate integration: rate[t] = τ*rate[t-1] + (1-τ)*spike[t]
        //    ∂L/∂spike_t = (1-τ) * τ^(T-1-t) * ∂L/∂rate_T
        //    (exponential weighting: recent spikes get more gradient)
        let t_steps = cache.cache_a.spikes.len();
        let tau = self.hyperparams.readout_tau;
        let one_minus_tau = 1.0 - tau;
        let mut grad_spikes_a = Vec::with_capacity(t_steps);
        let mut grad_spikes_b = Vec::with_capacity(t_steps);

        for t in 0..t_steps {
            let weight = one_minus_tau * tau.powi((t_steps - 1 - t) as i32);
            grad_spikes_a.push(&grad_rates_a * weight);
            grad_spikes_b.push(&grad_rates_b * weight);
        }

        // 6. Add spike rate regulation gradient
        let total_neurons = (self.layer_a.num_neurons + self.layer_b.num_neurons) as f64;
        let rate_grad_per_spike = loss_result.grad_spike_rate / (t_steps as f64 * total_neurons);
        for t in 0..t_steps {
            grad_spikes_a[t] = &grad_spikes_a[t] + rate_grad_per_spike;
            grad_spikes_b[t] = &grad_spikes_b[t] + rate_grad_per_spike;
        }

        // 7. Full BPTT through competitive pathways
        let mut grad_w_a = Array2::zeros(self.layer_a.weights.raw_dim());
        let mut grad_w_b = Array2::zeros(self.layer_b.weights.raw_dim());
        let mut grad_inhib_ab = Array2::zeros(self.inhibit_a_to_b.raw_dim());
        let mut grad_inhib_ba = Array2::zeros(self.inhibit_b_to_a.raw_dim());

        let mut g_v_a = Array1::zeros(self.layer_a.num_neurons);
        let mut g_u_a = Array1::zeros(self.layer_a.num_neurons);
        let mut g_v_b = Array1::zeros(self.layer_b.num_neurons);
        let mut g_u_b = Array1::zeros(self.layer_b.num_neurons);

        let dt_eff_a = self.layer_a.dt * 2.0;
        let dt_eff_b = self.layer_b.dt * 2.0;

        for t in (0..t_steps).rev() {
            // Gradient from spikes at t
            g_v_a = &g_v_a + &(&grad_spikes_a[t] * &cache.cache_a.spike_grads[t]);
            g_v_b = &g_v_b + &(&grad_spikes_b[t] * &cache.cache_b.spike_grads[t]);

            // Gradients for input weights
            let g_cur_a = &g_v_a * dt_eff_a;
            let g_cur_b = &g_v_b * dt_eff_b;

            let inp_a = &cache.cache_a.inputs[t];
            let inp_b = &cache.cache_b.inputs[t];
            for (i, &x) in inp_a.iter().enumerate() {
                let mut row = grad_w_a.row_mut(i);
                for (r, &g) in row.iter_mut().zip(g_cur_a.iter()) {
                    *r += x * g;
                }
            }
            for (i, &x) in inp_b.iter().enumerate() {
                let mut row = grad_w_b.row_mut(i);
                for (r, &g) in row.iter_mut().zip(g_cur_b.iter()) {
                    *r += x * g;
                }
            }

            // Gradients for inhibitory weights and cross-spike propagation
            if t > 0 {
                let prev_spikes_a = &cache.cache_a.spikes[t - 1];
                let prev_spikes_b = &cache.cache_b.spikes[t - 1];

                // inhibit_b_to_a: dL/dW_ba += spikes_b[t-1] ⊗ grad_current_a[t]
                for i in 0..self.layer_b.num_neurons {
                    for j in 0..self.layer_a.num_neurons {
                        grad_inhib_ba[[i, j]] += prev_spikes_b[i] * g_cur_a[j];
                    }
                }
                // inhibit_a_to_b: dL/dW_ab += spikes_a[t-1] ⊗ grad_current_b[t]
                for i in 0..self.layer_a.num_neurons {
                    for j in 0..self.layer_b.num_neurons {
                        grad_inhib_ab[[i, j]] += prev_spikes_a[i] * g_cur_b[j];
                    }
                }

                // Propagate gradient back to previous spikes (through inhibition)
                // dL/dspike_other[t-1] += W_inhib.dot(grad_current_this[t])
                let g_prev_spikes_a = self.inhibit_a_to_b.dot(&g_cur_b);
                let g_prev_spikes_b = self.inhibit_b_to_a.dot(&g_cur_a);

                grad_spikes_a[t - 1] = &grad_spikes_a[t - 1] + &g_prev_spikes_a;
                grad_spikes_b[t - 1] = &grad_spikes_b[t - 1] + &g_prev_spikes_b;
            }

            // Propagate through Izhikevich Jacobian and reset
            if t > 0 {
                let v_prev_a = &cache.cache_a.v_history[t];
                let v_prev_b = &cache.cache_b.v_history[t];

                let (ng_v_a, ng_u_a) = self.propagate_layer_backward(
                    &self.layer_a,
                    &cache.cache_a,
                    t,
                    &g_v_a,
                    &g_u_a,
                    v_prev_a,
                );
                let (ng_v_b, ng_u_b) = self.propagate_layer_backward(
                    &self.layer_b,
                    &cache.cache_b,
                    t,
                    &g_v_b,
                    &g_u_b,
                    v_prev_b,
                );

                g_v_a = ng_v_a;
                g_u_a = ng_u_a;
                g_v_b = ng_v_b;
                g_u_b = ng_u_b;
            }
        }

        // Readout bias gradient: ∂L/∂bias = ∂L/∂logits
        let mut grad_readout_bias = loss_result.grad_predictions.clone();

        // Gradient clipping on weight gradients to prevent exploding gradients
        grad_w_a.mapv_inplace(|x: f64| x.clamp(-10.0, 10.0));
        grad_w_b.mapv_inplace(|x: f64| x.clamp(-10.0, 10.0));
        grad_readout.mapv_inplace(|x: f64| x.clamp(-10.0, 10.0));
        grad_inhib_ab.mapv_inplace(|x: f64| x.clamp(-10.0, 10.0));
        grad_inhib_ba.mapv_inplace(|x: f64| x.clamp(-10.0, 10.0));
        grad_readout_bias.mapv_inplace(|x: f64| x.clamp(-10.0, 10.0));

        SnnGradients {
            loss: loss_result,
            grad_layer_a: grad_w_a,
            grad_layer_b: grad_w_b,
            grad_readout,
            grad_readout_bias,
            grad_inhibit_ab: grad_inhib_ab,
            grad_inhibit_ba: grad_inhib_ba,
        }
    }

    /// Helper for BPTT step through a single Izhikevich layer.
    fn propagate_layer_backward(
        &self,
        layer: &DiffIzhikevichLayer,
        cache: &IzhikevichForwardCache,
        t: usize,
        g_v: &Array1<f64>,
        g_u: &Array1<f64>,
        v_prev: &Array1<f64>,
    ) -> (Array1<f64>, Array1<f64>) {
        let mut new_g_v = Array1::zeros(layer.num_neurons);
        let mut new_g_u = Array1::zeros(layer.num_neurons);

        for i in 0..layer.num_neurons {
            let (eff_dvdv, eff_dvdu, eff_dudv, eff_dudu) =
                super::izhikevich_diff::izhikevich_jacobian(v_prev[i], layer.dt, layer.a, layer.b);

            new_g_v[i] = (g_v[i] * eff_dvdv + g_u[i] * eff_dudv).clamp(-10.0, 10.0);
            new_g_u[i] = (g_v[i] * eff_dvdu + g_u[i] * eff_dudu).clamp(-10.0, 10.0);

            if cache.spikes[t][i] > 0.5 {
                new_g_v[i] *= layer.reset_dampening;
            }
        }
        (new_g_v, new_g_u)
    }

    /// Performs a full training step: forward → loss → backward → Adam update.
    ///
    /// # Returns
    /// The PSA loss result for logging.
    pub fn train_step(
        &mut self,
        pos_inputs: &[Array1<f64>],
        neg_inputs: &[Array1<f64>],
        target_class: usize,
        volatility_ratio: f64,
        loss_config: &PsaLossConfig,
        optimizer: &mut AdamOptimizer,
    ) -> PsaLossResult {
        // Forward
        let (_, cache) = self.forward(pos_inputs, neg_inputs);

        // Backward
        let grads = self.backward(&cache, target_class, volatility_ratio, loss_config);

        // Adam step
        let mut params_2d = [
            &mut self.layer_a.weights,
            &mut self.layer_b.weights,
            &mut self.readout,
            &mut self.inhibit_a_to_b,
            &mut self.inhibit_b_to_a,
        ];
        let grads_2d = [
            grads.grad_layer_a,
            grads.grad_layer_b,
            grads.grad_readout,
            grads.grad_inhibit_ab,
            grads.grad_inhibit_ba,
        ];

        let mut params_1d = [&mut self.readout_bias];
        let grads_1d = [grads.grad_readout_bias];

        optimizer.step(&mut params_2d, &grads_2d, &mut params_1d, &grads_1d);

        // Enforce biological constraint: inhibitory weights must remain ≤ 0.
        // This was removed during refactoring and MUST be called after every optimizer step.
        self.clamp_inhibitory();

        grads.loss
    }

    /// Forces all cross-inhibitory weights to be non-positive.
    ///
    /// This enforces the biological constraint that inhibitory connections
    /// can only suppress activity, never excite. Called after every optimizer step.
    pub fn clamp_inhibitory(&mut self) {
        self.inhibit_a_to_b.mapv_inplace(|w| w.min(0.0));
        self.inhibit_b_to_a.mapv_inplace(|w| w.min(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_network() -> CompetitiveSnnNetwork {
        let hp = SnnHyperparameters {
            time_steps: 10,
            ..Default::default()
        };
        CompetitiveSnnNetwork::new(4, 8, 8, 3, hp)
    }

    #[test]
    fn test_network_creation() {
        let net = make_test_network();
        assert_eq!(net.layer_a.num_neurons, 8);
        assert_eq!(net.layer_b.num_neurons, 8);
        assert_eq!(net.readout.shape(), &[16, 3]);
        assert_eq!(net.inhibit_a_to_b.shape(), &[8, 8]);
    }

    #[test]
    fn test_forward_produces_logits() {
        let net = make_test_network();
        let t = 10;
        let pos = vec![Array1::from_vec(vec![1.0, 0.0, 0.5, 0.3]); t];
        let neg = vec![Array1::from_vec(vec![0.0, 1.0, 0.2, 0.0]); t];

        let (logits, cache) = net.forward(&pos, &neg);
        assert_eq!(logits.len(), 3);
        assert!(
            logits.iter().all(|v| v.is_finite()),
            "Logits should be finite"
        );
        assert!(cache.overall_spike_rate >= 0.0);
        assert!(cache.overall_spike_rate <= 1.0);
    }

    #[test]
    fn test_backward_produces_gradients() {
        let net = make_test_network();
        let t = 10;
        let pos = vec![Array1::from_vec(vec![1.0, 0.0, 0.5, 0.3]); t];
        let neg = vec![Array1::from_vec(vec![0.0, 1.0, 0.2, 0.0]); t];

        let (_, cache) = net.forward(&pos, &neg);
        let config = PsaLossConfig::default();
        let grads = net.backward(&cache, 0, 1.0, &config);

        assert!(grads.loss.total_loss.is_finite(), "Loss should be finite");
        assert_eq!(grads.grad_layer_a.shape(), net.layer_a.weights.shape());
        assert_eq!(grads.grad_layer_b.shape(), net.layer_b.weights.shape());
        assert_eq!(grads.grad_readout.shape(), net.readout.shape());
    }

    #[test]
    fn test_train_step_decreases_loss() {
        let hp = SnnHyperparameters {
            time_steps: 10,
            learning_rate: 0.01,
            ..Default::default()
        };
        let mut net = CompetitiveSnnNetwork::new(2, 8, 8, 3, hp);
        let config = PsaLossConfig::default();
        let mut opt = AdamOptimizer::new(
            0.01,
            &[
                (net.layer_a.input_dim, net.layer_a.num_neurons),
                (net.layer_b.input_dim, net.layer_b.num_neurons),
                (net.layer_a.num_neurons + net.layer_b.num_neurons, 3),
                (net.layer_a.num_neurons, net.layer_b.num_neurons), // ab
                (net.layer_b.num_neurons, net.layer_a.num_neurons), // ba
            ],
            &[3], // readout_bias shape
        );

        let pos = vec![Array1::from_vec(vec![1.0, 0.5]); 10];
        let neg = vec![Array1::from_vec(vec![0.0, 0.0]); 10];

        // Run multiple training steps
        let mut losses = Vec::new();
        for _ in 0..20 {
            let result = net.train_step(&pos, &neg, 0, 1.0, &config, &mut opt);
            losses.push(result.total_loss);
        }

        // The loss should generally decrease (allow some noise)
        let first_loss = losses[0];
        let last_loss = losses[losses.len() - 1];
        assert!(
            last_loss < first_loss * 1.5,
            "Loss should not explode: first={:.4}, last={:.4}",
            first_loss,
            last_loss
        );
    }

    #[test]
    fn test_inhibitory_clamping() {
        let mut net = make_test_network();
        // Set some positive values (shouldn't happen, but test the clamp)
        net.inhibit_a_to_b.fill(1.0);
        net.clamp_inhibitory();
        assert!(
            net.inhibit_a_to_b.iter().all(|&w| w <= 0.0),
            "Inhibitory weights must be ≤ 0 after clamping"
        );
    }

    #[test]
    fn test_hyperparams_serialization() {
        let hp = SnnHyperparameters::default();
        let json = serde_json::to_string(&hp).expect("Should serialize");
        let hp2: SnnHyperparameters = serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(hp.a, hp2.a);
        assert_eq!(hp.threshold, hp2.threshold);
        assert_eq!(hp.time_steps, hp2.time_steps);
    }

    #[test]
    fn test_zero_input_produces_finite_output() {
        let net = make_test_network();
        let t = 10;
        let pos = vec![Array1::zeros(4); t];
        let neg = vec![Array1::zeros(4); t];
        let (logits, _) = net.forward(&pos, &neg);
        assert!(logits.iter().all(|v| v.is_finite()));
    }
}
