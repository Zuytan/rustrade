//! Broker configuration parsing from environment variables.
//!
//! This module handles loading configuration for all supported brokers:
//! - Alpaca (Stock & Crypto)
//! - Binance (Crypto)
//! - OANDA (Forex)

use std::env;

/// Alpaca API configuration
#[derive(Debug, Clone, Default)]
pub struct AlpacaConfig {
    pub api_key: String,
    pub secret_key: String,
    pub base_url: String,
    pub data_url: String,
    pub ws_url: String,
}

impl AlpacaConfig {
    pub fn from_env() -> Self {
        Self {
            api_key: env::var("ALPACA_API_KEY").unwrap_or_default(),
            secret_key: env::var("ALPACA_SECRET_KEY").unwrap_or_default(),
            base_url: env::var("ALPACA_BASE_URL")
                .unwrap_or_else(|_| "https://paper-api.alpaca.markets".to_string()),
            data_url: env::var("ALPACA_DATA_URL")
                .unwrap_or_else(|_| "https://data.alpaca.markets".to_string()),
            ws_url: env::var("ALPACA_WS_URL")
                .unwrap_or_else(|_| "wss://stream.data.alpaca.markets/v2/iex".to_string()),
        }
    }
}

/// Binance API configuration
#[derive(Debug, Clone, Default)]
pub struct BinanceConfig {
    pub api_key: String,
    pub secret_key: String,
    pub base_url: String,
    pub ws_url: String,
}

impl BinanceConfig {
    pub fn from_env() -> Self {
        Self {
            api_key: env::var("BINANCE_API_KEY").unwrap_or_default(),
            secret_key: env::var("BINANCE_SECRET_KEY").unwrap_or_default(),
            base_url: env::var("BINANCE_BASE_URL")
                .unwrap_or_else(|_| "https://api.binance.com".to_string()),
            ws_url: env::var("BINANCE_WS_URL")
                .unwrap_or_else(|_| "wss://stream.binance.com:9443".to_string()),
        }
    }
}

/// OANDA API configuration
#[derive(Debug, Clone, Default)]
pub struct OandaConfig {
    pub api_base_url: String,
    pub stream_base_url: String,
    pub api_key: String,
    pub account_id: String,
}

impl OandaConfig {
    pub fn from_env() -> Self {
        Self {
            api_base_url: env::var("OANDA_API_BASE_URL")
                .unwrap_or_else(|_| "https://api-fxpractice.oanda.com".to_string()),
            stream_base_url: env::var("OANDA_STREAM_BASE_URL")
                .unwrap_or_else(|_| "https://stream-fxpractice.oanda.com".to_string()),
            api_key: env::var("OANDA_API_KEY").unwrap_or_default(),
            account_id: env::var("OANDA_ACCOUNT_ID").unwrap_or_default(),
        }
    }
}

/// Aggregated broker configuration
#[derive(Debug, Clone, Default)]
pub struct BrokerEnvConfig {
    pub alpaca: AlpacaConfig,
    pub binance: BinanceConfig,
    pub oanda: OandaConfig,
}

impl BrokerEnvConfig {
    pub fn from_env() -> Self {
        Self {
            alpaca: AlpacaConfig::from_env(),
            binance: BinanceConfig::from_env(),
            oanda: OandaConfig::from_env(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alpaca_config_defaults() {
        // Clear any existing env vars for test isolation
        let config = AlpacaConfig::from_env();
        assert!(config.base_url.contains("alpaca.markets"));
        assert!(config.data_url.contains("data.alpaca.markets"));
    }

    #[test]
    fn test_binance_config_defaults() {
        let config = BinanceConfig::from_env();
        assert!(config.base_url.contains("binance.com"));
    }

    #[test]
    fn test_oanda_config_defaults() {
        let config = OandaConfig::from_env();
        assert!(config.api_base_url.contains("oanda.com"));
    }
}
