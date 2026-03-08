// Modern strategies
mod ensemble;
pub mod statistical; // Modern statistical strategies
pub mod strategy_factory;
pub mod strategy_selector;
mod traits;

// Microstructure strategies (KEEP - these are modern)
pub mod ml_strategy;
mod order_flow;
mod smc;

#[cfg(test)]
mod qa;

#[cfg(test)]
mod tests;

// Modern strategies
pub use ensemble::EnsembleStrategy;
pub use ml_strategy::MLStrategy;
pub use order_flow::OrderFlowStrategy;
pub use smc::SMCStrategy;
pub use statistical::{StatisticalMomentumStrategy, ZScoreMeanReversionStrategy};
pub use strategy_factory::StrategyFactory;
pub use traits::{AnalysisContext, PositionInfo, Signal, TradingStrategy};
