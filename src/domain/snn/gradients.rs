use super::loss::PsaLossResult;
use ndarray::{Array1, Array2};

/// Struct holding gradients computed during SNN backpropagation.
#[derive(Debug, Clone)]
pub struct SnnGradients {
    pub loss: PsaLossResult,
    pub grad_layer_a: Array2<f64>,
    pub grad_layer_b: Array2<f64>,
    pub grad_readout: Array2<f64>,
    pub grad_readout_bias: Array1<f64>,
    pub grad_inhibit_ab: Array2<f64>,
    pub grad_inhibit_ba: Array2<f64>,
}
