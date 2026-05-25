use serde::{Deserialize, Serialize};

/// Non-differentiable hyperparameters tunable by Bayesian optimization.
///
/// These control the biophysics of the Izhikevich neurons and the
/// training dynamics. They are NOT updated by gradient descent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnnHyperparameters {
    /// Izhikevich parameter: time scale of recovery variable.
    pub a: f64,
    /// Izhikevich parameter: sensitivity of u to v.
    pub b: f64,
    /// Izhikevich parameter: after-spike reset value of v.
    pub c: f64,
    /// Izhikevich parameter: after-spike reset increment of u.
    pub d: f64,
    /// Spike threshold (mV).
    pub threshold: f64,
    /// Surrogate gradient steepness.
    pub surrogate_beta: f64,
    /// Adam learning rate.
    pub learning_rate: f64,
    /// Number of Euler integration steps per bar.
    pub time_steps: usize,
    /// Exponential decay for spike rate readout.
    pub readout_tau: f64,
    /// PSA loss: spike rate regulation weight.
    pub lambda_rate: f64,
    /// PSA loss: silence penalty weight.
    pub lambda_silence: f64,
    /// PSA loss: overtrade penalty weight.
    pub lambda_overtrade: f64,
}

impl Default for SnnHyperparameters {
    fn default() -> Self {
        Self {
            a: 0.02,
            b: 0.2,
            c: -65.0,
            d: 8.0,
            threshold: 30.0,
            surrogate_beta: 10.0,
            learning_rate: 0.001,
            time_steps: 50,
            readout_tau: 0.95,
            lambda_rate: 0.1,
            lambda_silence: 0.6,
            lambda_overtrade: 0.5,
        }
    }
}
