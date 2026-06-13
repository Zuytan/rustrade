pub mod core;
pub mod emergency;
pub mod execution;
pub mod position;
pub mod session;
pub mod validation;

// Re-exports so that existing imports like `use crate::application::risk_management::risk_manager` continue to work unchanged.
pub use core::risk_manager;
pub use core::state;

pub use validation::circuit_breaker_service;
pub use validation::commands;
pub use validation::pipeline;

pub use execution::order_monitor;
pub use execution::order_reconciler;
pub use execution::order_retry_strategy;
pub use execution::order_throttler;

pub use position::hard_stop_manager;
pub use position::portfolio_valuation_service;
pub use position::position_manager;
pub use position::sizing_engine;
pub use position::trailing_stops;

pub use session::session_manager;
pub use session::volatility;

pub use emergency::liquidation_service;
