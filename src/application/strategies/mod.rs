mod advanced;
mod dual_sma;
mod dynamic;
mod traits;
mod trend_riding;
mod mean_reversion;

pub use advanced::AdvancedTripleFilterStrategy;
pub use dual_sma::DualSMAStrategy;
pub use dynamic::DynamicRegimeStrategy;
pub use traits::{AnalysisContext, Signal, TradingStrategy};
pub use trend_riding::TrendRidingStrategy;
pub use mean_reversion::MeanReversionStrategy;
