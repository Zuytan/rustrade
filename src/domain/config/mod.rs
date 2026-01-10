//! Configuration domain module
//!
//! This module contains domain value objects for application configuration,
//! extracted from the monolithic Config struct to improve modularity and testability.

pub mod broker_config;
pub mod risk_config;
pub mod strategy_config;

pub use broker_config::{BrokerConfig, BrokerType};
pub use risk_config::RiskConfig;
pub use strategy_config::StrategyConfig;
