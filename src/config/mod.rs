//! Configuration module for Rustrade.
//!
//! This module provides structured configuration loading from environment variables,
//! organized by domain: Broker, Strategy, Risk, and Observability.

mod broker_config;
mod observability_config;
mod risk_env_config;
mod simulation_config;
mod strategy_config;

pub use broker_config::{AlpacaConfig, BinanceConfig, BrokerEnvConfig, OandaConfig};
pub use observability_config::ObservabilityEnvConfig;
pub use risk_env_config::{PlatformConfig, RiskEnvLoader};
pub use simulation_config::SimulationEnvConfig;
pub use strategy_config::StrategyEnvLoader;

// ... (imports remain)
pub use crate::domain::market::strategy_config::StrategyMode;
use anyhow::{Context, Result};
use std::env;
use std::str::FromStr;

/// Application execution mode
#[derive(Debug, Clone)]
pub enum Mode {
    Mock,
    Alpaca,
    Oanda,
    Binance,
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mock" => Ok(Mode::Mock),
            "alpaca" => Ok(Mode::Alpaca),
            "oanda" => Ok(Mode::Oanda),
            "binance" => Ok(Mode::Binance),
            _ => anyhow::bail!(
                "Invalid MODE: {}. Must be 'mock', 'alpaca', 'oanda', or 'binance'",
                s
            ),
        }
    }
}

/// Asset class for trading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetClass {
    Stock,
    Crypto,
}

impl FromStr for AssetClass {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stock" => Ok(AssetClass::Stock),
            "crypto" => Ok(AssetClass::Crypto),
            _ => anyhow::bail!("Invalid ASSET_CLASS: {}. Must be 'stock' or 'crypto'", s),
        }
    }
}

/// Main application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub asset_class: AssetClass,
    pub broker: BrokerEnvConfig,
    pub strategy: crate::domain::config::StrategyConfig,
    pub risk: crate::domain::config::RiskConfig,
    pub platform: PlatformConfig,
    pub observability: ObservabilityEnvConfig,
    pub simulation: SimulationEnvConfig,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let mode_str = env::var("MODE").unwrap_or_else(|_| "mock".to_string());
        let mode = Mode::from_str(&mode_str)?;

        let asset_class_str = env::var("ASSET_CLASS").unwrap_or_else(|_| "stock".to_string());
        let asset_class = AssetClass::from_str(&asset_class_str)?;

        // Load sub-configs
        let broker = BrokerEnvConfig::from_env();
        let strategy = StrategyEnvLoader::from_env().context("Failed to load strategy config")?;
        let (risk, platform) =
            RiskEnvLoader::from_env().context("Failed to load risk and platform config")?;
        let observability = ObservabilityEnvConfig::from_env();
        let simulation = SimulationEnvConfig::from_env();

        Ok(Self {
            mode,
            asset_class,
            broker,
            strategy,
            risk,
            platform,
            observability,
            simulation,
        })
    }

    pub fn create_fee_model(
        &self,
    ) -> std::sync::Arc<dyn crate::domain::trading::fee_model::FeeModel> {
        use crate::domain::trading::fee_model::{ConstantFeeModel, TieredFeeModel};

        match self.asset_class {
            AssetClass::Stock => std::sync::Arc::new(ConstantFeeModel::new(
                self.platform.commission_per_share,
                self.platform.slippage_pct,
            )),
            // Alpaca crypto: maker 0.15%, taker 0.25% (fallback — real fees fetched via API)
            AssetClass::Crypto => std::sync::Arc::new(TieredFeeModel::new(
                rust_decimal_macros::dec!(0.0015),
                rust_decimal_macros::dec!(0.0025),
                self.platform.slippage_pct,
            )),
        }
    }

    /// Create a RiskConfig domain value object from this Config
    pub fn to_risk_config(&self) -> Result<crate::domain::config::RiskConfig> {
        Ok(self.risk.clone())
    }

    /// Create a StrategyConfig domain value object from this Config
    pub fn to_strategy_config(&self) -> Result<crate::domain::config::StrategyConfig> {
        Ok(self.strategy.clone())
    }

    /// Create a BrokerConfig domain value object from this Config
    pub fn to_broker_config(&self) -> Result<crate::domain::config::BrokerConfig> {
        use crate::domain::config::BrokerType;

        let broker_type = match self.mode {
            Mode::Mock => BrokerType::Mock,
            Mode::Alpaca => BrokerType::Alpaca,
            Mode::Binance => BrokerType::Binance,
            Mode::Oanda => BrokerType::Oanda,
        };

        let (api_key, secret_key, base_url, ws_url, data_url) = match self.mode {
            Mode::Mock => (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                None,
            ),
            Mode::Alpaca => (
                self.broker.alpaca.api_key.clone(),
                self.broker.alpaca.secret_key.clone(),
                self.broker.alpaca.base_url.clone(),
                self.broker.alpaca.ws_url.clone(),
                Some(self.broker.alpaca.data_url.clone()),
            ),
            Mode::Binance => (
                self.broker.binance.api_key.clone(),
                self.broker.binance.secret_key.clone(),
                self.broker.binance.base_url.clone(),
                self.broker.binance.ws_url.clone(),
                None,
            ),
            Mode::Oanda => (
                self.broker.oanda.api_key.clone(),
                String::new(),
                self.broker.oanda.api_base_url.clone(),
                self.broker.oanda.stream_base_url.clone(),
                None,
            ),
        };

        crate::domain::config::BrokerConfig::new(
            broker_type,
            api_key,
            secret_key,
            base_url,
            ws_url,
            data_url,
        )
        .map_err(|e| anyhow::anyhow!("Invalid broker config: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env_defaults() {
        let config = Config::from_env().expect("Should parse with defaults");
        assert_eq!(config.risk.max_positions, 5);
        assert_eq!(config.strategy.fast_sma_period, 20); // Aligned with domain default
    }

    #[test]
    fn test_mode_parsing() {
        assert!(matches!(Mode::from_str("mock").unwrap(), Mode::Mock));
        assert!(matches!(Mode::from_str("ALPACA").unwrap(), Mode::Alpaca));
        assert!(Mode::from_str("invalid").is_err());
    }

    #[test]
    fn test_asset_class_parsing() {
        assert!(matches!(
            AssetClass::from_str("stock").unwrap(),
            AssetClass::Stock
        ));
        assert!(matches!(
            AssetClass::from_str("CRYPTO").unwrap(),
            AssetClass::Crypto
        ));
    }
}
