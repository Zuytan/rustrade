//! Surrogate Gradient Functions for Spiking Neural Networks.
//!
//! The spike generation function (Heaviside step) is non-differentiable.
//! During the backward pass of BPTT, we replace its gradient with a smooth
//! "surrogate" that approximates the shape of the Heaviside derivative (Dirac delta).
//!
//! This module provides:
//! - Forward: exact binary spike (Heaviside)
//! - Backward: smooth surrogate gradient (FastSigmoid or ATan)
//!
//! # References
//! - Neftci, Mostafa, Zenke (2019). "Surrogate Gradient Learning in Spiking Neural Networks"
//! - Zenke & Ganguli (2018). "SuperSpike: Supervised Learning in Multilayer Spiking Neural Networks"

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::fmt;

/// Available surrogate gradient functions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SurrogateType {
    /// Fast Sigmoid: σ(x) = 1 / (1 + β|x|)²
    /// Good general-purpose choice. Default β = 10.0.
    FastSigmoid,
    /// Arctangent: σ(x) = 1 / (π(1 + (πβx/2)²))
    /// Slightly wider gradient spread. Default β = 5.0.
    ATan,
    /// Triangle (Piecewise Linear): max(0, 1 - |x|/β)
    /// Very fast and effective for deep networks. Default β = 1.0.
    Triangle,
    /// Exponential: β * exp(-β|x|)
    /// Infinite support, avoids dead neurons. Default β = 5.0.
    Exponential,
}

impl fmt::Display for SurrogateType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SurrogateType::FastSigmoid => write!(f, "FastSigmoid"),
            SurrogateType::ATan => write!(f, "ATan"),
            SurrogateType::Triangle => write!(f, "Triangle"),
            SurrogateType::Exponential => write!(f, "Exponential"),
        }
    }
}

/// Binary Heaviside step function (exact forward pass).
///
/// Returns 1.0 if `v >= threshold`, 0.0 otherwise.
/// This is the true spike generation function used during inference.
#[inline(always)]
pub fn spike_forward(v: f64, threshold: f64) -> f64 {
    if v >= threshold { 1.0 } else { 0.0 }
}

/// Surrogate gradient for the Heaviside function (backward pass only).
///
/// Computes `∂spike/∂v` using a smooth approximation centered at `threshold`.
/// The `beta` parameter controls steepness: higher β → sharper peak, sparser gradients.
///
/// # Arguments
/// * `v` - Membrane potential
/// * `threshold` - Spike threshold (typically 30.0 for Izhikevich)
/// * `beta` - Steepness parameter
/// * `surrogate_type` - Which surrogate function to use
#[inline(always)]
pub fn spike_surrogate_grad(
    v: f64,
    threshold: f64,
    beta: f64,
    surrogate_type: SurrogateType,
) -> f64 {
    let x = v - threshold;
    match surrogate_type {
        SurrogateType::FastSigmoid => fast_sigmoid_grad(x, beta),
        SurrogateType::ATan => atan_grad(x, beta),
        SurrogateType::Triangle => triangle_grad(x, beta),
        SurrogateType::Exponential => exponential_grad(x, beta),
    }
}

/// Fast Sigmoid surrogate gradient.
///
/// `σ'(x) = 1 / (1 + β|x|)²`
///
/// Properties:
/// - Peak value at x=0: σ'(0) = 1.0
/// - Symmetric around threshold
/// - Decays as O(1/x²) — relatively narrow support
///
/// # Performance
/// Uses only addition, multiplication, and division — no transcendentals.
/// Ideal for hot-loop computation in HFT context.
#[inline(always)]
fn fast_sigmoid_grad(x: f64, beta: f64) -> f64 {
    let denom = 1.0 + beta * x.abs();
    1.0 / (denom * denom)
}

/// Arctangent surrogate gradient.
///
/// `σ'(x) = 1 / (π(1 + (πβx/2)²))`
///
/// Properties:
/// - Peak value at x=0: σ'(0) = 1/π ≈ 0.318
/// - Wider tails than FastSigmoid (Cauchy distribution shape)
/// - Better gradient flow for neurons far from threshold
#[inline(always)]
fn atan_grad(x: f64, beta: f64) -> f64 {
    let scaled = PI * beta * x / 2.0;
    1.0 / (PI * (1.0 + scaled * scaled))
}

/// Triangle (Piecewise Linear) surrogate gradient.
///
/// `σ'(x) = max(0, 1 - |x|/β)`
///
/// Properties:
/// - Fast to compute (no multiplication/division after pre-scaling)
/// - Strictly 0 outside the window [-β, β].
/// - At x=0, gradient is exactly 1.0.
#[inline(always)]
fn triangle_grad(x: f64, beta: f64) -> f64 {
    let abs_x = x.abs();
    // β acts as the width parameter here. E.g. β=1 means window is [-1, 1].
    let grad = (1.0 - abs_x / beta.max(1e-6)).max(0.0);
    grad.max(1e-4) // Leaky tail to prevent dead neurons
}

/// Exponential surrogate gradient.
///
/// `σ'(x) = β * exp(-β|x|)`
///
/// Properties:
/// - Infinite support (always > 0)
/// - Very strong peak at x=0.
#[inline(always)]
fn exponential_grad(x: f64, beta: f64) -> f64 {
    let grad = beta * (-beta * x.abs()).exp();
    grad.max(1e-4) // Leaky tail
}

/// Computes both the forward spike and the surrogate gradient in a single call.
///
/// # Returns
/// `(spike, grad)` where:
/// - `spike` is binary {0.0, 1.0} (exact Heaviside)
/// - `grad` is the smooth surrogate gradient ∂spike/∂v
///
/// This is the straight-through estimator: during forward, the output is binary;
/// during backward, the gradient is routed through the smooth surrogate.
#[inline(always)]
pub fn spike_with_grad(
    v: f64,
    threshold: f64,
    beta: f64,
    surrogate_type: SurrogateType,
) -> (f64, f64) {
    let spike = spike_forward(v, threshold);
    let grad = spike_surrogate_grad(v, threshold, beta, surrogate_type);
    (spike, grad)
}

/// Default steepness parameter for FastSigmoid surrogate.
pub const DEFAULT_FAST_SIGMOID_BETA: f64 = 10.0;

/// Default steepness parameter for ATan surrogate.
pub const DEFAULT_ATAN_BETA: f64 = 5.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spike_forward_fires() {
        assert_eq!(spike_forward(30.0, 30.0), 1.0);
        assert_eq!(spike_forward(35.0, 30.0), 1.0);
        assert_eq!(spike_forward(100.0, 30.0), 1.0);
    }

    #[test]
    fn test_spike_forward_no_fire() {
        assert_eq!(spike_forward(29.9, 30.0), 0.0);
        assert_eq!(spike_forward(-65.0, 30.0), 0.0);
        assert_eq!(spike_forward(0.0, 30.0), 0.0);
    }

    #[test]
    fn test_fast_sigmoid_grad_peak_at_threshold() {
        // At threshold, gradient should be maximal (1.0 for FastSigmoid)
        let grad = spike_surrogate_grad(30.0, 30.0, 10.0, SurrogateType::FastSigmoid);
        assert!(
            (grad - 1.0).abs() < 1e-10,
            "FastSigmoid grad at threshold should be 1.0, got {}",
            grad
        );
    }

    #[test]
    fn test_fast_sigmoid_grad_decays_away_from_threshold() {
        let grad_at = spike_surrogate_grad(30.0, 30.0, 10.0, SurrogateType::FastSigmoid);
        let grad_near = spike_surrogate_grad(30.5, 30.0, 10.0, SurrogateType::FastSigmoid);
        let grad_far = spike_surrogate_grad(35.0, 30.0, 10.0, SurrogateType::FastSigmoid);

        assert!(
            grad_at > grad_near,
            "Gradient should decrease away from threshold"
        );
        assert!(
            grad_near > grad_far,
            "Gradient should decrease further away"
        );
        assert!(
            grad_far > 0.0,
            "Gradient should remain positive (never exactly zero)"
        );
    }

    #[test]
    fn test_fast_sigmoid_grad_symmetry() {
        let grad_above = spike_surrogate_grad(31.0, 30.0, 10.0, SurrogateType::FastSigmoid);
        let grad_below = spike_surrogate_grad(29.0, 30.0, 10.0, SurrogateType::FastSigmoid);
        assert!(
            (grad_above - grad_below).abs() < 1e-10,
            "FastSigmoid should be symmetric around threshold"
        );
    }

    #[test]
    fn test_atan_grad_peak_at_threshold() {
        let grad = spike_surrogate_grad(30.0, 30.0, 5.0, SurrogateType::ATan);
        let expected = 1.0 / PI; // ATan peak is 1/π
        assert!(
            (grad - expected).abs() < 1e-10,
            "ATan grad at threshold should be 1/π ≈ {:.6}, got {:.6}",
            expected,
            grad
        );
    }

    #[test]
    fn test_atan_grad_decay() {
        // ATan surrogate should peak at threshold and decay monotonically
        let grad_at = spike_surrogate_grad(30.0, 30.0, 5.0, SurrogateType::ATan);
        let grad_near = spike_surrogate_grad(31.0, 30.0, 5.0, SurrogateType::ATan);
        let grad_far = spike_surrogate_grad(40.0, 30.0, 5.0, SurrogateType::ATan);

        assert!(grad_at > grad_near, "ATan should peak at threshold");
        assert!(
            grad_near > grad_far,
            "ATan should decay away from threshold"
        );
        assert!(
            grad_far > 0.0,
            "ATan should have infinite support (non-zero)"
        );
    }

    #[test]
    fn test_spike_with_grad_straight_through() {
        // Below threshold: spike=0, but grad should be non-zero
        let (spike, grad) = spike_with_grad(
            29.5,
            30.0,
            DEFAULT_FAST_SIGMOID_BETA,
            SurrogateType::FastSigmoid,
        );
        assert_eq!(spike, 0.0, "Spike should not fire below threshold");
        assert!(
            grad > 0.0,
            "Surrogate gradient should be non-zero near threshold"
        );

        // Above threshold: spike=1, and grad should be non-zero
        let (spike, grad) = spike_with_grad(
            30.5,
            30.0,
            DEFAULT_FAST_SIGMOID_BETA,
            SurrogateType::FastSigmoid,
        );
        assert_eq!(spike, 1.0, "Spike should fire above threshold");
        assert!(
            grad > 0.0,
            "Surrogate gradient should be non-zero near threshold"
        );
    }

    #[test]
    fn test_beta_steepness_controls_gradient_width() {
        // Higher beta → narrower gradient
        let grad_low_beta = spike_surrogate_grad(31.0, 30.0, 1.0, SurrogateType::FastSigmoid);
        let grad_high_beta = spike_surrogate_grad(31.0, 30.0, 100.0, SurrogateType::FastSigmoid);
        assert!(
            grad_low_beta > grad_high_beta,
            "Lower beta should give wider (larger) gradients at distance: low={:.6}, high={:.8}",
            grad_low_beta,
            grad_high_beta
        );
    }

    #[test]
    fn test_triangle_grad() {
        // peak at 0
        let grad = spike_surrogate_grad(30.0, 30.0, 1.0, SurrogateType::Triangle);
        assert_eq!(grad, 1.0);

        // edge of window (leaky)
        let grad_edge = spike_surrogate_grad(31.0, 30.0, 1.0, SurrogateType::Triangle);
        assert_eq!(grad_edge, 1e-4);

        // outside window should be leaky
        let grad_out = spike_surrogate_grad(28.0, 30.0, 1.0, SurrogateType::Triangle);
        assert_eq!(grad_out, 1e-4);

        // inside window
        let grad_in = spike_surrogate_grad(30.5, 30.0, 1.0, SurrogateType::Triangle);
        assert_eq!(grad_in, 0.5);

        let grad_in_wide = spike_surrogate_grad(31.0, 30.0, 2.0, SurrogateType::Triangle);
        assert_eq!(grad_in_wide, 0.5);
    }

    #[test]
    fn test_exponential_grad() {
        let beta = 2.0;
        let grad_at = spike_surrogate_grad(30.0, 30.0, beta, SurrogateType::Exponential);
        assert_eq!(grad_at, 2.0); // beta * exp(0)

        let grad_near = spike_surrogate_grad(31.0, 30.0, beta, SurrogateType::Exponential);
        let expected = beta * (-beta * 1.0_f64).exp();
        assert!((grad_near - expected).abs() < 1e-10);

        // symmetric check
        let grad_below = spike_surrogate_grad(29.0, 30.0, beta, SurrogateType::Exponential);
        assert_eq!(grad_near, grad_below);
    }
}
