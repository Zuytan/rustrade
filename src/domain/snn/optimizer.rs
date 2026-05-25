//! Adam Optimizer for ndarray-based neural network parameters.
//!
//! Minimal, zero-dependency Adam implementation operating directly on `ndarray::Array2<f64>`.
//! Follows the original paper: Kingma & Ba (2015), "Adam: A Method for Stochastic Optimization".
//!
//! # Design Choice
//! Rather than pulling in a full ML framework for optimization, we implement Adam
//! directly. For an SNN with O(100) neurons, the optimizer overhead is negligible
//! compared to the BPTT computation. This keeps the dependency graph clean.

use ndarray::{Array1, Array2};

/// Adam optimizer state for a set of parameter tensors (2D and 1D).
#[derive(Debug, Clone)]
pub struct AdamOptimizer {
    /// Learning rate (α). Typical: 1e-3.
    pub lr: f64,
    /// Exponential decay rate for the first moment (β₁). Typical: 0.9.
    pub beta1: f64,
    /// Exponential decay rate for the second moment (β₂). Typical: 0.999.
    pub beta2: f64,
    /// Small constant for numerical stability (ε). Typical: 1e-8.
    pub epsilon: f64,
    /// Global timestep counter (for bias correction).
    pub t: usize,

    /// First moment estimates for 2D params.
    m2: Vec<Array2<f64>>,
    /// Second moment estimates for 2D params.
    v2: Vec<Array2<f64>>,
    /// First moment estimates for 1D params.
    m1: Vec<Array1<f64>>,
    /// Second moment estimates for 1D params.
    v1: Vec<Array1<f64>>,
}

impl AdamOptimizer {
    /// Creates a new Adam optimizer.
    ///
    /// # Arguments
    /// * `lr` - Learning rate
    /// * `param_shapes_2d` - Shapes of each 2D parameter tensor (rows, cols).
    /// * `param_shapes_1d` - Lengths of each 1D parameter tensor.
    pub fn new(lr: f64, param_shapes_2d: &[(usize, usize)], param_shapes_1d: &[usize]) -> Self {
        let m2: Vec<Array2<f64>> = param_shapes_2d
            .iter()
            .map(|&(r, c)| Array2::zeros((r, c)))
            .collect();
        let v2: Vec<Array2<f64>> = param_shapes_2d
            .iter()
            .map(|&(r, c)| Array2::zeros((r, c)))
            .collect();
        let m1: Vec<Array1<f64>> = param_shapes_1d.iter().map(|&l| Array1::zeros(l)).collect();
        let v1: Vec<Array1<f64>> = param_shapes_1d.iter().map(|&l| Array1::zeros(l)).collect();

        Self {
            lr,
            beta1: 0.9,
            beta2: 0.999,
            epsilon: 1e-10,
            t: 0,
            m2,
            v2,
            m1,
            v1,
        }
    }

    /// Performs a single optimization step (in-place parameter update).
    pub fn step(
        &mut self,
        params_2d: &mut [&mut Array2<f64>],
        grads_2d: &[Array2<f64>],
        params_1d: &mut [&mut Array1<f64>],
        grads_1d: &[Array1<f64>],
    ) {
        self.t += 1;

        // Bias correction factors
        let bc1 = 1.0 - self.beta1.powi(self.t as i32);
        let bc2 = 1.0 - self.beta2.powi(self.t as i32);

        // 2D Parameters update
        for i in 0..params_2d.len() {
            let grad = &grads_2d[i];
            self.m2[i] = &self.m2[i] * self.beta1 + grad * (1.0 - self.beta1);
            self.v2[i] = &self.v2[i] * self.beta2 + &(grad * grad) * (1.0 - self.beta2);

            let m_hat = &self.m2[i] / bc1;
            let v_hat = &self.v2[i] / bc2;

            let update = &m_hat / &(v_hat.mapv(f64::sqrt) + self.epsilon);
            let new_val = &**params_2d[i] - &update * self.lr;
            params_2d[i].assign(&new_val);
        }

        // 1D Parameters update
        for i in 0..params_1d.len() {
            let grad = &grads_1d[i];
            self.m1[i] = &self.m1[i] * self.beta1 + grad * (1.0 - self.beta1);
            self.v1[i] = &self.v1[i] * self.beta2 + &(grad * grad) * (1.0 - self.beta2);

            let m_hat = &self.m1[i] / bc1;
            let v_hat = &self.v1[i] / bc2;

            let update = &m_hat / &(v_hat.mapv(f64::sqrt) + self.epsilon);
            let new_val = &**params_1d[i] - &update * self.lr;
            params_1d[i].assign(&new_val);
        }
    }

    /// Resets the optimizer state (moments and timestep).
    pub fn reset(&mut self) {
        self.t = 0;
        for m in &mut self.m2 {
            m.fill(0.0);
        }
        for v in &mut self.v2 {
            v.fill(0.0);
        }
        for m in &mut self.m1 {
            m.fill(0.0);
        }
        for v in &mut self.v1 {
            v.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adam_creation() {
        let opt = AdamOptimizer::new(0.001, &[(10, 5), (5, 3)], &[3]);
        assert_eq!(opt.m2.len(), 2);
        assert_eq!(opt.v2.len(), 2);
        assert_eq!(opt.m1.len(), 1);
        assert_eq!(opt.m2[0].shape(), &[10, 5]);
        assert_eq!(opt.m1[0].len(), 3);
        assert_eq!(opt.t, 0);
    }

    #[test]
    fn test_adam_step_updates_params() {
        let mut param = Array2::from_elem((3, 2), 1.0);
        let grad = Array2::from_elem((3, 2), 0.1);
        let mut bias = Array1::from_elem(3, 1.0);
        let g_bias = Array1::from_elem(3, 0.1);

        let mut opt = AdamOptimizer::new(0.01, &[(3, 2)], &[3]);
        opt.step(&mut [&mut param], &[grad], &mut [&mut bias], &[g_bias]);

        // After one step, params should have moved
        assert!(param.iter().all(|&p| (p - 1.0).abs() > 1e-10));
        assert!(bias.iter().all(|&b| (b - 1.0).abs() > 1e-10));
        assert_eq!(opt.t, 1);
    }

    #[test]
    fn test_adam_moves_toward_zero() {
        let mut param = Array2::from_elem((2, 2), 5.0);
        let grad = Array2::from_elem((2, 2), 1.0);

        let mut opt = AdamOptimizer::new(0.1, &[(2, 2)], &[]);

        for _ in 0..100 {
            opt.step(&mut [&mut param], std::slice::from_ref(&grad), &mut [], &[]);
        }

        assert!(param.iter().all(|&p| p < 5.0));
    }

    #[test]
    fn test_adam_reset() {
        let mut opt = AdamOptimizer::new(0.01, &[(3, 2)], &[3]);
        let mut param = Array2::from_elem((3, 2), 1.0);
        let grad = Array2::from_elem((3, 2), 0.1);
        let mut bias = Array1::from_elem(3, 1.0);
        let g_bias = Array1::from_elem(3, 0.1);

        opt.step(&mut [&mut param], &[grad], &mut [&mut bias], &[g_bias]);
        assert_eq!(opt.t, 1);

        opt.reset();
        assert_eq!(opt.t, 0);
        assert!(opt.m2[0].iter().all(|&m| m == 0.0));
        assert!(opt.m1[0].iter().all(|&m| m == 0.0));
    }

    #[test]
    fn test_adam_bias_correction() {
        let mut param = Array2::from_elem((1, 1), 0.0);
        let grad = Array2::from_elem((1, 1), 1.0);

        let mut opt = AdamOptimizer::new(0.01, &[(1, 1)], &[]);
        opt.step(&mut [&mut param], &[grad], &mut [], &[]);

        assert!((param[[0, 0]] + 0.01).abs() < 1e-6);
    }
}
