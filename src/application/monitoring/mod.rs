pub mod correlation_service;
pub mod health;
pub mod portfolio;
pub mod strategy;

// Re-export modules so that references like `use crate::application::monitoring::agent_status` continue to work unchanged.
pub use health::agent_status;
pub use health::connection_health_service;
pub use health::heartbeat;

pub use portfolio::cost_evaluator;
pub use portfolio::performance_monitoring_service;
pub use portfolio::portfolio_state_manager;

pub use strategy::empirical_win_rate_provider;
pub use strategy::feature_engineering_service;
pub use strategy::strategy_validator;
