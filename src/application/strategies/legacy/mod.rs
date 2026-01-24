// DEPRECATED: Legacy strategies from 1980-90s era
// These strategies are kept for backward compatibility and A/B testing
// but are NOT recommended for production use.
//
// Modern alternatives:
// - DualSMA -> Use StatisticalMomentum or ZScoreMR
// - Advanced -> Use ML-based strategies (Phase 3)
// - MeanReversion -> Use ZScoreMR
// - TrendRiding -> Use StatisticalMomentum
// - Breakout -> Use SMC or OrderFlow
// - Momentum -> Use StatisticalMomentum
// - VWAP -> Use ZScoreMR with volume confirmation

pub(crate) mod advanced;
pub(crate) mod breakout;
pub(crate) mod dual_sma;
pub(crate) mod mean_reversion;
pub(crate) mod momentum;
pub(crate) mod trend_riding;
pub(crate) mod vwap;

#[deprecated(
    since = "0.86.0",
    note = "Use StatisticalMomentumStrategy or ZScoreMeanReversionStrategy instead"
)]
pub use dual_sma::DualSMAStrategy;

#[deprecated(since = "0.86.0", note = "Use ML-based strategies instead")]
pub use advanced::{AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy};

#[deprecated(since = "0.86.0", note = "Use ZScoreMeanReversionStrategy instead")]
pub use mean_reversion::MeanReversionStrategy;

#[deprecated(since = "0.86.0", note = "Use StatisticalMomentumStrategy instead")]
pub use trend_riding::TrendRidingStrategy;

#[deprecated(
    since = "0.86.0",
    note = "Use SMCStrategy or OrderFlowStrategy instead"
)]
pub use breakout::BreakoutStrategy;

#[deprecated(since = "0.86.0", note = "Use StatisticalMomentumStrategy instead")]
pub use momentum::MomentumDivergenceStrategy;

#[deprecated(
    since = "0.86.0",
    note = "Use ZScoreMeanReversionStrategy with volume confirmation instead"
)]
pub use vwap::VWAPStrategy;

// Re-export traits for convenience
pub use super::traits::{AnalysisContext, Signal, TradingStrategy};
