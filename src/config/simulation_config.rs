use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct SimulationEnvConfig {
    pub simulation_enabled: bool,
    pub simulation_latency_base_ms: u64,
    pub simulation_latency_jitter_ms: u64,
    pub simulation_slippage_volatility: f64,
}

impl SimulationEnvConfig {
    pub fn from_env() -> Self {
        let simulation_enabled = env::var("SIMULATION_ENABLED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(false);

        let simulation_latency_base_ms = env::var("SIMULATION_LATENCY_BASE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50); // Default 50ms base latency

        let simulation_latency_jitter_ms = env::var("SIMULATION_LATENCY_JITTER_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20); // Default 20ms jitter

        let simulation_slippage_volatility = env::var("SIMULATION_SLIPPAGE_VOLATILITY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0005); // Default 5bps volatility

        Self {
            simulation_enabled,
            simulation_latency_base_ms,
            simulation_latency_jitter_ms,
            simulation_slippage_volatility,
        }
    }
}
