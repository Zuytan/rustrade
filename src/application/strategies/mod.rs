// Legacy strategies (DEPRECATED - use statistical/ or microstructure/ instead)
pub mod legacy;

// Modern strategies
mod dynamic;
mod ensemble;
pub mod statistical; // Modern statistical strategies
pub mod strategy_factory;
pub mod strategy_selector;
mod traits;

// Microstructure strategies (KEEP - these are modern)
pub mod ml_strategy;
mod order_flow;
mod smc;

// Re-export legacy strategies with deprecation warnings
#[allow(deprecated)]
pub use legacy::{
    AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy, BreakoutStrategy, DualSMAStrategy,
    MeanReversionStrategy, MomentumDivergenceStrategy, TrendRidingStrategy, VWAPStrategy,
};

// Modern strategies
pub use dynamic::{DynamicRegimeConfig, DynamicRegimeStrategy};
pub use ensemble::EnsembleStrategy;
pub use ml_strategy::MLStrategy;
pub use order_flow::OrderFlowStrategy;
pub use smc::SMCStrategy;
pub use statistical::{StatisticalMomentumStrategy, ZScoreMeanReversionStrategy};
pub use strategy_factory::StrategyFactory;
pub use traits::{AnalysisContext, PositionInfo, Signal, TradingStrategy};
