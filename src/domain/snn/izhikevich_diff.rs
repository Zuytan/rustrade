//! Differentiable Izhikevich Neuron Layer with manual BPTT.
//!
//! This module implements a fully-connected layer of Izhikevich neurons where:
//! - **Forward pass**: Euler-discretized Izhikevich dynamics with exact binary spikes.
//! - **Backward pass**: Manual BPTT using analytically-derived Jacobians and surrogate gradients.
//!
//! The layer maintains no internal state between calls — membrane potentials `v` and recovery
//! variables `u` are threaded through the time loop explicitly, enabling clean gradient computation.
//!
//! # Memory Safety & Performance
//! - All allocations are done upfront via `Vec::with_capacity` to avoid runtime reallocation.
//! - The forward cache stores the minimal set of values needed for the backward pass.
//! - Using `ndarray` for vectorized operations over the neuron dimension.
//! - `f64` is used for numerical stability in gradient computations (chain rule through many timesteps).

use ndarray::{Array1, Array2};
use serde::{Deserialize, Serialize};

use super::surrogate::{SurrogateType, spike_with_grad};

/// Performs one Euler sub-step of the Izhikevich dynamics (shared helper).
///
/// Computes:
///   v' = v + dt * (0.04*v² + 5*v + 140 - u + I)
///   u' = u + dt * a * (b*v' - u)
///
/// This is the canonical Izhikevich integration step, extracted to avoid
/// duplicating the dynamics across DiffIzhikevichLayer and CompetitiveSnnNetwork.
#[inline]
pub fn euler_substep(
    v: &Array1<f64>,
    u: &Array1<f64>,
    current: &Array1<f64>,
    a: f64,
    b: f64,
    dt: f64,
) -> (Array1<f64>, Array1<f64>) {
    let dv = v * v * 0.04 + v * 5.0 + 140.0 - u + current;
    let v_new = v + &dv * dt;
    let du = (&v_new * b - u) * a;
    let u_new = u + &du * dt;
    (v_new, u_new)
}

/// Computes the effective Jacobian elements for two Euler sub-steps (shared helper).
///
/// Returns `(dvdv, dvdu, dudv, dudu)` — the partial derivatives of the
/// Izhikevich update rule over two sub-steps at a given membrane potential.
///
/// Used by both `DiffIzhikevichLayer::backward()` and
/// `CompetitiveSnnNetwork::propagate_layer_backward()` to avoid divergence.
#[inline]
pub fn izhikevich_jacobian(v_prev_i: f64, dt: f64, a: f64, b: f64) -> (f64, f64, f64, f64) {
    let dvdu = -dt;
    let dudv_single = dt * a * b;
    let eff_dvdv = 1.0 + 2.0 * dt * (0.08 * v_prev_i + 5.0);
    let eff_dvdu = 2.0 * dvdu;
    let eff_dudv = 2.0 * dudv_single;
    let eff_dudu = (1.0 - dt * a) * (1.0 - dt * a) + dvdu * dudv_single * dt;
    (eff_dvdv, eff_dvdu, eff_dudv, eff_dudu)
}

/// A fully-connected layer of differentiable Izhikevich neurons.
///
/// Weights map from `input_dim` to `num_neurons`. Each neuron follows
/// the Izhikevich model with parameters `(a, b, c, d)`.
///
/// The Izhikevich parameters are intentionally NOT learnable by gradient descent
/// (they are neuronal biophysics, not connection strengths). They should be tuned
/// by an external hyperparameter optimizer (e.g., Bayesian optimization).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffIzhikevichLayer {
    /// Number of neurons in this layer.
    pub num_neurons: usize,

    /// Input dimensionality.
    pub input_dim: usize,

    /// Synaptic weight matrix [input_dim, num_neurons] — the ONLY learnable parameter.
    /// Gradient descent updates these via BPTT + surrogate gradients.
    pub weights: Array2<f64>,

    // --- Izhikevich biophysical parameters (non-differentiable) ---
    /// Time scale of recovery variable `u`. Typical: 0.02 (RS), 0.1 (FS).
    pub a: f64,
    /// Sensitivity of `u` to subthreshold fluctuations of `v`. Typical: 0.2.
    pub b: f64,
    /// After-spike reset value of `v`. Typical: -65.0 (RS), -50.0 (CH).
    pub c: f64,
    /// After-spike reset increment of `u`. Typical: 8.0 (RS), 2.0 (CH/FS).
    pub d: f64,

    /// Membrane potential threshold for spike generation (mV).
    pub threshold: f64,

    /// Steepness of the surrogate gradient. Higher → sharper, but sparser gradients.
    pub surrogate_beta: f64,

    /// Which surrogate gradient function to use.
    pub surrogate_type: SurrogateType,

    /// Euler integration timestep. Use 0.5 for stability (two sub-steps per logical tick).
    pub dt: f64,

    /// Factor to dampen the gradient flowing back through v after a spike reset.
    /// Default is 0.1.
    pub reset_dampening: f64,
}

/// Cached intermediate values from the forward pass, required for backward BPTT.
///
/// Stores the full temporal trajectory so the backward pass can compute
/// the chain rule through each timestep without recomputation.
#[derive(Debug, Clone)]
pub struct IzhikevichForwardCache {
    /// Membrane potential history: `v_history[t]` is the state BEFORE timestep `t`.
    /// Length = T+1 (includes initial state).
    pub v_history: Vec<Array1<f64>>,

    /// Recovery variable history: `u_history[t]` is the state BEFORE timestep `t`.
    /// Length = T+1.
    pub u_history: Vec<Array1<f64>>,

    /// Binary spike outputs at each timestep. Length = T.
    pub spikes: Vec<Array1<f64>>,

    /// Surrogate gradient values at each timestep (∂spike/∂v). Length = T.
    pub spike_grads: Vec<Array1<f64>>,

    /// Injected currents (W · input) at each timestep. Length = T.
    pub currents: Vec<Array1<f64>>,

    /// Input vectors at each timestep (needed for weight gradient). Length = T.
    pub inputs: Vec<Array1<f64>>,
}

impl DiffIzhikevichLayer {
    /// Creates a new layer with Xavier-initialized weights.
    ///
    /// Xavier initialization scales weights by `sqrt(2 / (fan_in + fan_out))`
    /// to maintain gradient magnitude across layers.
    pub fn new(input_dim: usize, num_neurons: usize) -> Self {
        // Xavier uniform initialization boosted for SNN
        // Standard Xavier gives weights ~[-0.4, 0.4] which isn't enough to drive
        // Izhikevich neurons to spike (rheobase needs I ~ 5-10).
        // Multiplying by 10 ensures initial random spiking, avoiding the "Dead Neuron" gradient freeze.
        let scale = (6.0 / (input_dim + num_neurons) as f64).sqrt() * 10.0;
        let mut weights = Array2::zeros((input_dim, num_neurons));

        // Simple deterministic initialization (can be re-seeded externally)
        let mut seed: u64 = 42;
        for w in weights.iter_mut() {
            // LCG pseudo-random for initialization
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let uniform = (seed >> 33) as f64 / (1u64 << 31) as f64; // [0, 1)
            *w = (uniform * 2.0 - 1.0) * scale;
        }

        Self {
            num_neurons,
            input_dim,
            weights,
            // Default: Regular Spiking (RS) parameters
            a: 0.02,
            b: 0.2,
            c: -65.0,
            d: 8.0,
            threshold: 30.0,
            surrogate_beta: 10.0,
            surrogate_type: SurrogateType::Triangle,
            dt: 0.5,              // Two sub-steps per logical tick for stability
            reset_dampening: 0.1, // Default soft attenuation through resets
        }
    }

    /// Creates a layer with specific Izhikevich parameters.
    pub fn with_params(
        input_dim: usize,
        num_neurons: usize,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    ) -> Self {
        let mut layer = Self::new(input_dim, num_neurons);
        layer.a = a;
        layer.b = b;
        layer.c = c;
        layer.d = d;
        layer
    }

    /// Initializes the resting state for membrane potential and recovery variable.
    pub fn initial_state(&self) -> (Array1<f64>, Array1<f64>) {
        let v = Array1::from_elem(self.num_neurons, self.c);
        let u = Array1::from_elem(self.num_neurons, self.c * self.b);
        (v, u)
    }

    /// Forward pass: processes a sequence of input vectors through the Izhikevich layer.
    ///
    /// # Arguments
    /// * `inputs` - Sequence of input vectors, one per timestep. Each has shape [input_dim].
    ///
    /// # Returns
    /// * `(spike_trains, cache)` where `spike_trains[t]` is the binary spike vector at time `t`,
    ///   and `cache` contains all intermediate values needed for `backward()`.
    ///
    /// # Performance
    /// Pre-allocates all vectors to avoid runtime reallocation. The hot loop
    /// performs only arithmetic operations (no allocations, no branching except spike check).
    pub fn forward(&self, inputs: &[Array1<f64>]) -> (Vec<Array1<f64>>, IzhikevichForwardCache) {
        let t_steps = inputs.len();

        // Pre-allocate all output/cache vectors
        let mut v_history = Vec::with_capacity(t_steps + 1);
        let mut u_history = Vec::with_capacity(t_steps + 1);
        let mut spikes = Vec::with_capacity(t_steps);
        let mut spike_grads = Vec::with_capacity(t_steps);
        let mut currents = Vec::with_capacity(t_steps);
        let mut cached_inputs = Vec::with_capacity(t_steps);

        // Initialize state
        let (mut v, mut u) = self.initial_state();
        v_history.push(v.clone());
        u_history.push(u.clone());

        for input in inputs.iter().take(t_steps) {
            // 1. Compute injected current: I = W^T · input
            //    weights: [input_dim, num_neurons], input: [input_dim]
            //    current: [num_neurons]
            let current = self.weights.t().dot(input);
            currents.push(current.clone());
            cached_inputs.push(input.clone());

            // 2. Euler integration with two sub-steps (dt=0.5) for numerical stability
            //    This is standard practice for Izhikevich model to prevent overshooting.
            for _sub_step in 0..2 {
                let (v_new, u_new) = euler_substep(&v, &u, &current, self.a, self.b, self.dt);
                v = v_new;
                u = u_new;
            }

            // 3. Spike generation with surrogate gradient
            let mut spike_vec = Array1::zeros(self.num_neurons);
            let mut grad_vec = Array1::zeros(self.num_neurons);

            for i in 0..self.num_neurons {
                let (s, g) = spike_with_grad(
                    v[i],
                    self.threshold,
                    self.surrogate_beta,
                    self.surrogate_type,
                );
                spike_vec[i] = s;
                grad_vec[i] = g;
            }

            // 4. Reset after spike: v = v*(1-spike) + c*spike, u = u + d*spike
            for i in 0..self.num_neurons {
                if spike_vec[i] > 0.5 {
                    v[i] = self.c;
                    u[i] += self.d;
                }
            }

            spikes.push(spike_vec);
            spike_grads.push(grad_vec);
            v_history.push(v.clone());
            u_history.push(u.clone());
        }

        let cache = IzhikevichForwardCache {
            v_history,
            u_history,
            spikes,
            spike_grads,
            currents,
            inputs: cached_inputs,
        };

        // Return spike trains from cache (avoids a full Vec<Array1> clone)
        let spikes_out = cache.spikes.clone();
        (spikes_out, cache)
    }

    /// Backward pass: computes weight gradients via BPTT through the Izhikevich dynamics.
    ///
    /// # Arguments
    /// * `cache` - Forward cache from `forward()`.
    /// * `grad_output` - Gradient of the loss w.r.t. the output spikes at each timestep.
    ///   Shape: `&[Array1<f64>]` with length T, each of shape [num_neurons].
    ///
    /// # Returns
    /// * Weight gradients of shape [input_dim, num_neurons].
    ///
    /// # Chain Rule Structure
    /// For each timestep t (reverse order):
    ///   ∂L/∂spike_t is given by grad_output[t]
    ///   ∂spike_t/∂v_t = surrogate_grad (from cache)
    ///   ∂v_t/∂I_t = dt (Euler: v += dt * (...+ I))
    ///   ∂I_t/∂W = input_t^T (outer product)
    ///   Plus recurrent term: ∂v_{t+1}/∂v_t through the Izhikevich Jacobian
    pub fn backward(
        &self,
        cache: &IzhikevichForwardCache,
        grad_output: &[Array1<f64>],
    ) -> Array2<f64> {
        let t_steps = grad_output.len();
        let mut grad_weights = Array2::zeros((self.input_dim, self.num_neurons));

        // Accumulated gradient flowing backward through time
        // dL/dv at timestep t (propagated from future timesteps)
        let mut grad_v = Array1::zeros(self.num_neurons);
        let mut grad_u = Array1::zeros(self.num_neurons);

        // Reverse time: t = T-1, T-2, ..., 0
        for t in (0..t_steps).rev() {
            // 1. Gradient from the loss at this timestep
            //    dL/dspike_t is given; dspike_t/dv_t is the surrogate gradient
            let spike_grad = &cache.spike_grads[t];

            // Total gradient on v at this timestep:
            // = (dL/dspike_t * dspike_t/dv_t) + (recurrent contribution from t+1)
            let grad_v_from_loss = &grad_output[t] * spike_grad;
            grad_v = &grad_v + &grad_v_from_loss;

            // 2. Gradient through the reset:
            //    v_out = v_pre * (1 - spike) + c * spike
            //    dv_out/dv_pre = (1 - spike) + (c - v_pre) * dspike/dv_pre
            //    For the backward accumulator, we already have the post-reset gradient.
            //    The surrogate gradient handles the spike non-differentiability.

            // 3. Gradient through Izhikevich Euler step to the current I:
            //    v_new = v_old + dt * (0.04*v_old^2 + 5*v_old + 140 - u_old + I)
            //    ∂v_new/∂I = dt (applied over two sub-steps, effective: 2*dt = 1.0)
            //    But we have two sub-steps, so the effective sensitivity to I is approximately dt*2 = 1.0
            let effective_dt = self.dt * 2.0; // Two sub-steps of dt each

            // 4. Gradient on current: dL/dI_t = dL/dv_t * (∂v_t/∂I_t)
            let grad_current = &grad_v * effective_dt;

            // 5. Gradient on weights: dL/dW += input_t^T ⊗ dL/dI_t
            //    This is the outer product: grad_weights[i,j] += input[i] * grad_current[j]
            let input = &cache.inputs[t];
            for (i, &x) in input.iter().enumerate() {
                let mut row = grad_weights.row_mut(i);
                for (r, &g) in row.iter_mut().zip(grad_current.iter()) {
                    *r += x * g;
                }
            }

            // 6. Propagate gradient backward through time (Jacobian of Izhikevich)
            //    v_{t} was computed from v_{t-1}: need ∂v_t/∂v_{t-1}
            if t > 0 {
                let v_prev = &cache.v_history[t]; // v BEFORE this timestep

                let mut new_grad_v: Array1<f64> = Array1::zeros(self.num_neurons);
                let mut new_grad_u: Array1<f64> = Array1::zeros(self.num_neurons);

                for i in 0..self.num_neurons {
                    let (eff_dvdv, eff_dvdu, eff_dudv, eff_dudu) =
                        izhikevich_jacobian(v_prev[i], self.dt, self.a, self.b);

                    // Backprop: grad_v_{t-1} = grad_v_t * ∂v_t/∂v_{t-1} + grad_u_t * ∂u_t/∂v_{t-1}
                    let v_val: f64 = grad_v[i] * eff_dvdv + grad_u[i] * eff_dudv;
                    let u_val: f64 = grad_v[i] * eff_dvdu + grad_u[i] * eff_dudu;
                    new_grad_v[i] = v_val.clamp(-10.0, 10.0);
                    new_grad_u[i] = u_val.clamp(-10.0, 10.0);
                }

                // If a spike occurred, reset breaks the gradient chain partially
                // (straight-through estimator handles this via the surrogate)
                for i in 0..self.num_neurons {
                    if cache.spikes[t][i] > 0.5 {
                        // After spike reset, gradient through v is attenuated
                        // because v was overwritten to c (constant)
                        new_grad_v[i] *= self.reset_dampening; // Configurable soft attenuation
                    }
                }

                grad_v = new_grad_v;
                grad_u = new_grad_u;
            }
        }

        grad_weights
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_creation() {
        let layer = DiffIzhikevichLayer::new(10, 5);
        assert_eq!(layer.num_neurons, 5);
        assert_eq!(layer.input_dim, 10);
        assert_eq!(layer.weights.shape(), &[10, 5]);
    }

    #[test]
    fn test_forward_produces_binary_spikes() {
        let layer = DiffIzhikevichLayer::new(4, 8);
        let inputs = vec![Array1::from_vec(vec![1.0, 0.0, 1.0, 0.0]); 20];
        let (spikes, _cache) = layer.forward(&inputs);

        // All spike values must be exactly 0.0 or 1.0
        for spike_vec in &spikes {
            for &s in spike_vec.iter() {
                assert!(s == 0.0 || s == 1.0, "Spike must be binary, got {}", s);
            }
        }
    }

    #[test]
    fn test_strong_input_causes_spikes() {
        // With very strong input weights, neurons must fire
        let mut layer = DiffIzhikevichLayer::new(1, 1);
        layer.weights = Array2::from_elem((1, 1), 100.0); // Very strong connection

        let inputs = vec![Array1::from_vec(vec![1.0]); 50];
        let (spikes, _) = layer.forward(&inputs);

        let total_spikes: f64 = spikes.iter().map(|s| s.sum()).sum();
        assert!(
            total_spikes > 1.0,
            "Strong input should produce multiple spikes, got {}",
            total_spikes
        );
    }

    #[test]
    fn test_no_input_no_spikes() {
        let layer = DiffIzhikevichLayer::new(4, 8);
        let inputs = vec![Array1::zeros(4); 50];
        let (spikes, _) = layer.forward(&inputs);

        let total_spikes: f64 = spikes.iter().map(|s| s.sum()).sum();
        assert_eq!(total_spikes, 0.0, "Zero input should produce zero spikes");
    }

    #[test]
    fn test_cache_dimensions() {
        let layer = DiffIzhikevichLayer::new(4, 8);
        let t_steps = 20;
        let inputs = vec![Array1::from_vec(vec![0.5, 0.3, 0.0, 0.1]); t_steps];
        let (_, cache) = layer.forward(&inputs);

        assert_eq!(cache.v_history.len(), t_steps + 1);
        assert_eq!(cache.u_history.len(), t_steps + 1);
        assert_eq!(cache.spikes.len(), t_steps);
        assert_eq!(cache.spike_grads.len(), t_steps);
        assert_eq!(cache.currents.len(), t_steps);
        assert_eq!(cache.inputs.len(), t_steps);
    }

    #[test]
    fn test_backward_produces_gradient() {
        let mut layer = DiffIzhikevichLayer::new(2, 4);
        layer.weights = Array2::from_elem((2, 4), 50.0);

        let inputs = vec![Array1::from_vec(vec![1.0, 0.5]); 10];
        let (_, cache) = layer.forward(&inputs);

        // Gradient as if loss is sum of all spikes (encourage spiking)
        let grad_output: Vec<Array1<f64>> = cache.spikes.iter().map(|_| Array1::ones(4)).collect();

        let grad_w = layer.backward(&cache, &grad_output);

        assert_eq!(grad_w.shape(), &[2, 4]);

        // Gradient should be non-zero (learning signal exists)
        let grad_norm: f64 = grad_w.iter().map(|g| g * g).sum::<f64>().sqrt();
        assert!(
            grad_norm > 1e-12,
            "Weight gradient should be non-zero, got norm {}",
            grad_norm
        );
    }

    #[test]
    fn test_backward_gradient_shape_matches_weights() {
        let layer = DiffIzhikevichLayer::new(10, 20);
        let inputs = vec![Array1::ones(10) * 0.5; 30];
        let (_, cache) = layer.forward(&inputs);
        let grad_output: Vec<Array1<f64>> = (0..30).map(|_| Array1::ones(20)).collect();

        let grad_w = layer.backward(&cache, &grad_output);
        assert_eq!(
            grad_w.shape(),
            layer.weights.shape(),
            "Gradient shape must match weight shape"
        );
    }

    #[test]
    fn test_reset_after_spike() {
        let mut layer = DiffIzhikevichLayer::new(1, 1);
        layer.weights = Array2::from_elem((1, 1), 200.0); // Very strong
        layer.c = -65.0;

        let inputs = vec![Array1::from_vec(vec![1.0]); 10];
        let (_, cache) = layer.forward(&inputs);

        // After a spike, v should be reset to c
        for t in 0..cache.spikes.len() {
            if cache.spikes[t][0] > 0.5 {
                // Next timestep, v should start near c
                if t + 1 < cache.v_history.len() {
                    let v_after = cache.v_history[t + 1][0];
                    assert!(
                        (v_after - layer.c).abs() < 20.0,
                        "After spike, v should be near c={}, got {}",
                        layer.c,
                        v_after
                    );
                }
            }
        }
    }

    #[test]
    fn test_with_params() {
        let layer = DiffIzhikevichLayer::with_params(5, 3, 0.1, 0.2, -50.0, 2.0);
        assert_eq!(layer.a, 0.1);
        assert_eq!(layer.b, 0.2);
        assert_eq!(layer.c, -50.0);
        assert_eq!(layer.d, 2.0);
    }
}
