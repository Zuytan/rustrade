// Risk management and position control modules
pub mod circuit_breaker_service; // New
pub mod commands;
pub mod liquidation_service;
pub mod order_reconciler;
pub mod order_throttler;
pub mod pipeline;
pub mod portfolio_valuation_service;
pub mod position_manager;
pub mod risk_manager;
pub mod session_manager;
pub mod sizing_engine;
pub mod state;
pub mod trailing_stops; // New
