//! Observability configuration parsing from environment variables.
//!
//! This module handles loading monitoring and metrics configuration.

use std::env;

/// Observability environment configuration
#[derive(Debug, Clone)]
pub struct ObservabilityEnvConfig {
    pub enabled: bool,
    pub port: u16,
    pub bind_address: String,
}

impl Default for ObservabilityEnvConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9090,
            bind_address: "127.0.0.1".to_string(),
        }
    }
}

impl ObservabilityEnvConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: env::var("OBSERVABILITY_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .parse::<bool>()
                .unwrap_or(true),
            port: env::var("OBSERVABILITY_PORT")
                .unwrap_or_else(|_| "9090".to_string())
                .parse::<u16>()
                .unwrap_or(9090),
            bind_address: env::var("OBSERVABILITY_BIND_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observability_config_defaults() {
        let config = ObservabilityEnvConfig::from_env();
        assert!(config.enabled);
        assert_eq!(config.port, 9090);
        assert_eq!(config.bind_address, "127.0.0.1");
    }
}
