//! Broker Configuration Domain Value Object
//!
//! This module defines the `BrokerConfig` value object for broker-specific settings.

use thiserror::Error;

/// Error type for BrokerConfig validation
#[derive(Debug, Error, PartialEq)]
pub enum BrokerConfigError {
    #[error("Empty API key")]
    EmptyApiKey,
    
    #[error("Empty secret key")]
    EmptySecretKey,
    
    #[error("Invalid URL: {field}")]
    InvalidUrl { field: String },
}

/// Broker type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerType {
    Mock,
    Alpaca,
    Binance,
    Oanda,
}

/// Broker configuration value object
///
/// # Invariants
///
/// - For non-Mock brokers: API keys must not be empty
/// - All URLs must be non-empty
#[derive(Debug, Clone, PartialEq)]
pub struct BrokerConfig {
    pub broker_type: BrokerType,
    pub api_key: String,
    pub secret_key: String,
    pub base_url: String,
    pub ws_url: String,
    pub data_url: Option<String>, // For Alpaca data API
}

impl BrokerConfig {
    /// Create a new BrokerConfig with validation
    pub fn new(
        broker_type: BrokerType,
        api_key: String,
        secret_key: String,
        base_url: String,
        ws_url: String,
        data_url: Option<String>,
    ) -> Result<Self, BrokerConfigError> {
        let config = Self {
            broker_type,
            api_key,
            secret_key,
            base_url,
            ws_url,
            data_url,
        };
        
        config.validate()?;
        Ok(config)
    }
    
    fn validate(&self) -> Result<(), BrokerConfigError> {
        // Mock broker doesn't need credentials
        if !matches!(self.broker_type, BrokerType::Mock) {
            if self.api_key.is_empty() {
                return Err(BrokerConfigError::EmptyApiKey);
            }
            if self.secret_key.is_empty() {
                return Err(BrokerConfigError::EmptySecretKey);
            }
        }
        
        // URLs must not be empty for non-Mock
        if !matches!(self.broker_type, BrokerType::Mock) {
            if self.base_url.is_empty() {
                return Err(BrokerConfigError::InvalidUrl {
                    field: "base_url".to_string(),
                });
            }
            if self.ws_url.is_empty() {
                return Err(BrokerConfigError::InvalidUrl {
                    field: "ws_url".to_string(),
                });
            }
        }
        
        Ok(())
    }
    
    /// Create a mock broker config for testing
    pub fn mock() -> Self {
        Self {
            broker_type: BrokerType::Mock,
            api_key: String::new(),
            secret_key: String::new(),
            base_url: String::new(),
            ws_url: String::new(),
            data_url: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_alpaca_config() {
        let config = BrokerConfig::new(
            BrokerType::Alpaca,
            "test_key".to_string(),
            "test_secret".to_string(),
            "https://paper-api.alpaca.markets".to_string(),
            "wss://stream.data.alpaca.markets".to_string(),
            Some("https://data.alpaca.markets".to_string()),
        );
        assert!(config.is_ok());
    }

    #[test]
    fn test_mock_config() {
        let config = BrokerConfig::mock();
        assert_eq!(config.broker_type, BrokerType::Mock);
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_empty_api_key() {
        let result = BrokerConfig::new(
            BrokerType::Alpaca,
            String::new(), // Empty
            "secret".to_string(),
            "https://api.example.com".to_string(),
            "wss://ws.example.com".to_string(),
            None,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), BrokerConfigError::EmptyApiKey);
    }

    #[test]
    fn test_empty_secret_key() {
        let result = BrokerConfig::new(
            BrokerType::Binance,
            "key".to_string(),
            String::new(), // Empty
            "https://api.binance.com".to_string(),
            "wss://stream.binance.com".to_string(),
            None,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), BrokerConfigError::EmptySecretKey);
    }

    #[test]
    fn test_empty_base_url() {
        let result = BrokerConfig::new(
            BrokerType::Alpaca,
            "key".to_string(),
            "secret".to_string(),
            String::new(), // Empty
            "wss://ws.example.com".to_string(),
            None,
        );
        assert!(result.is_err());
    }
}
