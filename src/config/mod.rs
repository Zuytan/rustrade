//! Configuration module for Rustrade.
//!
//! This module provides structured configuration loading from environment variables,
//! organized by domain: Broker, Strategy, Risk, and Observability.

mod api_config;
mod broker_config;
mod observability_config;
mod risk_env_config;
mod simulation_config;
mod strategy_config;

pub use api_config::ApiServerConfig;
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
use tracing::info;

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

/// Returns the broker URL that would trade real money in this mode, or `None` for
/// mock mode and sandbox endpoints.
///
/// Fail-closed: a URL is production unless it carries the broker's sandbox marker
/// (`paper-api` for Alpaca, `testnet` for Binance, `fxpractice` for OANDA).
pub(crate) fn live_trading_target<'a>(mode: &Mode, broker: &'a BrokerEnvConfig) -> Option<&'a str> {
    let (url, sandbox_marker) = match mode {
        Mode::Mock => return None,
        Mode::Alpaca => (broker.alpaca.base_url.as_str(), "paper-api"),
        Mode::Binance => (broker.binance.base_url.as_str(), "testnet"),
        Mode::Oanda => (broker.oanda.api_base_url.as_str(), "fxpractice"),
    };
    if url.to_lowercase().contains(sandbox_marker) {
        None
    } else {
        Some(url)
    }
}

/// Only an explicit `true` (case-insensitive, surrounding whitespace ignored) confirms live trading.
pub(crate) fn is_live_confirmed(live_flag: Option<&str>) -> bool {
    live_flag.is_some_and(|flag| flag.trim().eq_ignore_ascii_case("true"))
}

/// Main application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub asset_class: AssetClass,
    pub broker: BrokerEnvConfig,
    pub strategy: crate::domain::config::StrategyConfig,
    pub risk: crate::domain::risk::risk_config::RiskConfig,
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

        let config = Self {
            mode,
            asset_class,
            broker,
            strategy,
            risk,
            platform,
            observability,
            simulation,
        };

        // Safety gate: prevent accidental live trading
        config.validate_live_trading_gate(env::var("TRADING_LIVE").ok().as_deref())?;

        Ok(config)
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

    /// Validates that live trading is intentionally enabled for every broker.
    ///
    /// Any broker URL without a known sandbox marker is treated as production (fail-closed),
    /// and then requires `TRADING_LIVE=true`. This prevents accidental trading with real money,
    /// including through proxies or custom endpoints.
    fn validate_live_trading_gate(&self, live_flag: Option<&str>) -> Result<()> {
        let Some(url) = live_trading_target(&self.mode, &self.broker) else {
            return Ok(());
        };

        if !is_live_confirmed(live_flag) {
            let (broker, env_var, sandbox_url) = match self.mode {
                Mode::Alpaca => (
                    "Alpaca",
                    "ALPACA_BASE_URL",
                    "https://paper-api.alpaca.markets",
                ),
                Mode::Binance => (
                    "Binance",
                    "BINANCE_BASE_URL",
                    "https://testnet.binance.vision",
                ),
                Mode::Oanda => (
                    "OANDA",
                    "OANDA_API_BASE_URL",
                    "https://api-fxpractice.oanda.com",
                ),
                Mode::Mock => return Ok(()),
            };
            anyhow::bail!(
                "🚨 LIVE TRADING GATE: {} URL points to PRODUCTION ({}). \
                 Set TRADING_LIVE=true to confirm you intend to trade with real money. \
                 If this is unintentional, change {} to {}",
                broker,
                url,
                env_var,
                sandbox_url
            );
        }

        info!(
            "⚠️  LIVE TRADING MODE ENABLED — Trading with real money on {}",
            url
        );
        Ok(())
    }

    /// Create a RiskConfig domain value object from this Config
    pub fn to_risk_config(&self) -> Result<crate::domain::risk::risk_config::RiskConfig> {
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

    fn broker_with(alpaca: &str, binance: &str, oanda: &str) -> BrokerEnvConfig {
        BrokerEnvConfig {
            alpaca: AlpacaConfig {
                base_url: alpaca.to_string(),
                ..Default::default()
            },
            binance: BinanceConfig {
                base_url: binance.to_string(),
                ..Default::default()
            },
            oanda: OandaConfig {
                api_base_url: oanda.to_string(),
                ..Default::default()
            },
        }
    }

    const ALPACA_PAPER: &str = "https://paper-api.alpaca.markets";
    const ALPACA_LIVE: &str = "https://api.alpaca.markets";
    const BINANCE_TEST: &str = "https://testnet.binance.vision";
    const BINANCE_LIVE: &str = "https://api.binance.com";
    const OANDA_PRACTICE: &str = "https://api-fxpractice.oanda.com";
    const OANDA_LIVE: &str = "https://api-fxtrade.oanda.com";

    #[test]
    fn test_live_target_sandbox_urls_are_not_live() {
        let broker = broker_with(ALPACA_PAPER, BINANCE_TEST, OANDA_PRACTICE);
        assert_eq!(live_trading_target(&Mode::Alpaca, &broker), None);
        assert_eq!(live_trading_target(&Mode::Binance, &broker), None);
        assert_eq!(live_trading_target(&Mode::Oanda, &broker), None);
    }

    #[test]
    fn test_live_target_production_urls_are_live() {
        let broker = broker_with(ALPACA_LIVE, BINANCE_LIVE, OANDA_LIVE);
        assert_eq!(
            live_trading_target(&Mode::Alpaca, &broker),
            Some(ALPACA_LIVE)
        );
        assert_eq!(
            live_trading_target(&Mode::Binance, &broker),
            Some(BINANCE_LIVE)
        );
        assert_eq!(live_trading_target(&Mode::Oanda, &broker), Some(OANDA_LIVE));
    }

    #[test]
    fn test_live_target_unknown_url_is_treated_as_live() {
        // A proxy or custom endpoint without a sandbox marker must fail closed.
        let proxy = "https://broker-proxy.internal";
        let broker = broker_with(proxy, proxy, proxy);
        assert_eq!(live_trading_target(&Mode::Alpaca, &broker), Some(proxy));
        assert_eq!(live_trading_target(&Mode::Binance, &broker), Some(proxy));
        assert_eq!(live_trading_target(&Mode::Oanda, &broker), Some(proxy));
    }

    #[test]
    fn test_live_target_sandbox_marker_is_case_insensitive() {
        let broker = broker_with(
            "HTTPS://PAPER-API.ALPACA.MARKETS",
            "https://TESTNET.binance.vision",
            "https://API-FXPRACTICE.oanda.com",
        );
        assert_eq!(live_trading_target(&Mode::Alpaca, &broker), None);
        assert_eq!(live_trading_target(&Mode::Binance, &broker), None);
        assert_eq!(live_trading_target(&Mode::Oanda, &broker), None);
    }

    #[test]
    fn test_live_target_mock_is_never_live() {
        let broker = broker_with(ALPACA_LIVE, BINANCE_LIVE, OANDA_LIVE);
        assert_eq!(live_trading_target(&Mode::Mock, &broker), None);
    }

    #[test]
    fn test_live_flag_parsing() {
        assert!(!is_live_confirmed(None));
        assert!(!is_live_confirmed(Some("false")));
        assert!(!is_live_confirmed(Some("1")));
        assert!(!is_live_confirmed(Some("")));
        assert!(is_live_confirmed(Some("true")));
        assert!(is_live_confirmed(Some("TRUE")));
        assert!(is_live_confirmed(Some(" true ")));
    }

    #[test]
    fn test_gate_blocks_live_binance_without_flag() {
        let mut config = Config::from_env().expect("Should parse with defaults");
        config.mode = Mode::Binance;
        config.broker = broker_with(ALPACA_PAPER, BINANCE_LIVE, OANDA_PRACTICE);

        let err = config
            .validate_live_trading_gate(None)
            .expect_err("live Binance without TRADING_LIVE must be refused");
        let msg = err.to_string();
        assert!(msg.contains("Binance"));
        assert!(msg.contains("BINANCE_BASE_URL"));
        assert!(config.validate_live_trading_gate(Some("true")).is_ok());
    }

    #[test]
    fn test_gate_allows_sandbox_without_flag() {
        let mut config = Config::from_env().expect("Should parse with defaults");
        config.broker = broker_with(ALPACA_PAPER, BINANCE_TEST, OANDA_PRACTICE);
        for mode in [Mode::Mock, Mode::Alpaca, Mode::Binance, Mode::Oanda] {
            config.mode = mode;
            assert!(config.validate_live_trading_gate(None).is_ok());
        }
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
