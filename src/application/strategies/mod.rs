// Modern strategies
mod ensemble;
pub mod statistical; // Modern statistical strategies
pub mod strategy_factory;
pub mod strategy_selector;
mod traits;

// Microstructure strategies (KEEP - these are modern)
mod order_flow;
mod smc;
pub mod snn_surrogate_strategy;

#[cfg(test)]
mod qa;

#[cfg(test)]
mod tests;

// Modern strategies
pub use ensemble::EnsembleStrategy;
pub use order_flow::OrderFlowStrategy;
pub use smc::SMCStrategy;
pub use snn_surrogate_strategy::SnnSurrogateStrategy;
pub use statistical::{StatisticalMomentumStrategy, ZScoreMeanReversionStrategy};
pub use strategy_factory::StrategyFactory;
pub use traits::{AnalysisContext, PositionInfo, Signal, TradingStrategy};
