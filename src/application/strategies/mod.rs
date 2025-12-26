mod traits;
mod dual_sma;
mod advanced;
mod dynamic;
mod trend_riding;

pub use traits::{AnalysisContext, Signal, TradingStrategy};
pub use dual_sma::DualSMAStrategy;
pub use advanced::AdvancedTripleFilterStrategy;
pub use dynamic::DynamicRegimeStrategy;
pub use trend_riding::TrendRidingStrategy;
