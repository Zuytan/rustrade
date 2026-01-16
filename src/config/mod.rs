//! Configuration module for Rustrade.
//!
//! This module provides structured configuration loading from environment variables,
//! organized by domain: Broker, Strategy, Risk, and Observability.

mod broker_config;
mod observability_config;
mod risk_env_config;
mod strategy_config;

pub use broker_config::{AlpacaConfig, BinanceConfig, BrokerEnvConfig, OandaConfig};
pub use observability_config::ObservabilityEnvConfig;
pub use risk_env_config::RiskEnvConfig;
pub use strategy_config::StrategyEnvConfig;

// Re-export StrategyMode for backward compatibility
pub use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::market::timeframe::Timeframe;
use crate::domain::risk::risk_appetite::RiskAppetite;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use std::collections::HashMap;
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
///
/// This struct aggregates all configuration from sub-modules and provides
/// backward-compatible field access for the rest of the application.
#[derive(Debug, Clone)]
pub struct Config {
    // Core
    pub mode: Mode,
    pub asset_class: AssetClass,

    // Broker (from BrokerEnvConfig)
    pub alpaca_api_key: String,
    pub alpaca_secret_key: String,
    pub alpaca_base_url: String,
    pub alpaca_data_url: String,
    pub alpaca_ws_url: String,
    pub oanda_api_base_url: String,
    pub oanda_stream_base_url: String,
    pub oanda_api_key: String,
    pub oanda_account_id: String,
    pub binance_api_key: String,
    pub binance_secret_key: String,
    pub binance_base_url: String,
    pub binance_ws_url: String,

    // Strategy (from StrategyEnvConfig)
    pub fast_sma_period: usize,
    pub slow_sma_period: usize,
    pub trend_sma_period: usize,
    pub sma_threshold: f64,
    pub rsi_period: usize,
    pub rsi_threshold: f64,
    pub macd_fast_period: usize,
    pub macd_slow_period: usize,
    pub macd_signal_period: usize,
    pub macd_requires_rising: bool,
    pub macd_min_threshold: f64,
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,
    pub adx_period: usize,
    pub adx_threshold: f64,
    pub atr_period: usize,
    pub trailing_stop_atr_multiplier: f64,
    pub strategy_mode: StrategyMode,
    pub trend_divergence_threshold: f64,
    pub trend_tolerance_pct: f64,
    pub mean_reversion_rsi_exit: f64,
    pub mean_reversion_bb_period: usize,
    pub trend_riding_exit_buffer_pct: f64,
    pub smc_ob_lookback: usize,
    pub smc_min_fvg_size_pct: f64,
    pub primary_timeframe: Timeframe,
    pub enabled_timeframes: Vec<Timeframe>,
    pub trend_timeframe: Timeframe,
    pub signal_confirmation_bars: usize,
    pub take_profit_pct: f64,
    pub profit_target_multiplier: f64,

    // Risk (from RiskEnvConfig)
    pub max_positions: usize,
    pub max_position_size_pct: f64,
    pub max_position_value_usd: f64,
    pub risk_per_trade_percent: f64,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub consecutive_loss_limit: usize,
    pub pending_order_ttl_ms: Option<i64>,
    pub max_sector_exposure_pct: f64,
    pub sector_map: HashMap<String, String>,
    pub non_pdt_mode: bool,
    pub max_orders_per_minute: u32,
    pub order_cooldown_seconds: u64,
    pub min_hold_time_minutes: i64,
    pub slippage_pct: f64,
    pub commission_per_share: f64,
    pub spread_bps: f64,
    pub min_profit_ratio: f64,
    pub initial_cash: Decimal,
    pub trade_quantity: Decimal,
    pub portfolio_staleness_ms: u64,
    pub portfolio_refresh_interval_ms: u64,
    pub dynamic_symbol_mode: bool,
    pub dynamic_scan_interval_minutes: u64,
    pub symbols: Vec<String>,
    pub min_volume_threshold: f64,
    pub adaptive_optimization_enabled: bool,
    pub regime_detection_window: usize,
    pub adaptive_evaluation_hour: u32,
    pub risk_appetite: Option<RiskAppetite>,

    // Observability (from ObservabilityEnvConfig)
    pub observability_enabled: bool,
    pub observability_port: u16,
    pub observability_bind_address: String,
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// This orchestrates loading from all sub-config modules and composes
    /// them into a unified Config struct.
    pub fn from_env() -> Result<Self> {
        // Core settings
        let mode_str = env::var("MODE").unwrap_or_else(|_| "mock".to_string());
        let mode = Mode::from_str(&mode_str)?;

        let asset_class_str = env::var("ASSET_CLASS").unwrap_or_else(|_| "stock".to_string());
        let asset_class = AssetClass::from_str(&asset_class_str)?;

        // Load sub-configs
        let broker = BrokerEnvConfig::from_env();
        let strategy = StrategyEnvConfig::from_env().context("Failed to load strategy config")?;
        let risk = RiskEnvConfig::from_env().context("Failed to load risk config")?;
        let observability = ObservabilityEnvConfig::from_env();

        Ok(Self {
            mode,
            asset_class,

            // Broker
            alpaca_api_key: broker.alpaca.api_key,
            alpaca_secret_key: broker.alpaca.secret_key,
            alpaca_base_url: broker.alpaca.base_url,
            alpaca_data_url: broker.alpaca.data_url,
            alpaca_ws_url: broker.alpaca.ws_url,
            oanda_api_base_url: broker.oanda.api_base_url,
            oanda_stream_base_url: broker.oanda.stream_base_url,
            oanda_api_key: broker.oanda.api_key,
            oanda_account_id: broker.oanda.account_id,
            binance_api_key: broker.binance.api_key,
            binance_secret_key: broker.binance.secret_key,
            binance_base_url: broker.binance.base_url,
            binance_ws_url: broker.binance.ws_url,

            // Strategy
            fast_sma_period: strategy.fast_sma_period,
            slow_sma_period: strategy.slow_sma_period,
            trend_sma_period: strategy.trend_sma_period,
            sma_threshold: strategy.sma_threshold,
            rsi_period: strategy.rsi_period,
            rsi_threshold: strategy.rsi_threshold,
            macd_fast_period: strategy.macd_fast_period,
            macd_slow_period: strategy.macd_slow_period,
            macd_signal_period: strategy.macd_signal_period,
            macd_requires_rising: strategy.macd_requires_rising,
            macd_min_threshold: strategy.macd_min_threshold,
            ema_fast_period: strategy.ema_fast_period,
            ema_slow_period: strategy.ema_slow_period,
            adx_period: strategy.adx_period,
            adx_threshold: strategy.adx_threshold,
            atr_period: strategy.atr_period,
            trailing_stop_atr_multiplier: strategy.trailing_stop_atr_multiplier,
            strategy_mode: strategy.strategy_mode,
            trend_divergence_threshold: strategy.trend_divergence_threshold,
            trend_tolerance_pct: strategy.trend_tolerance_pct,
            mean_reversion_rsi_exit: strategy.mean_reversion_rsi_exit,
            mean_reversion_bb_period: strategy.mean_reversion_bb_period,
            trend_riding_exit_buffer_pct: strategy.trend_riding_exit_buffer_pct,
            smc_ob_lookback: strategy.smc_ob_lookback,
            smc_min_fvg_size_pct: strategy.smc_min_fvg_size_pct,
            primary_timeframe: strategy.primary_timeframe,
            enabled_timeframes: strategy.enabled_timeframes,
            trend_timeframe: strategy.trend_timeframe,
            signal_confirmation_bars: strategy.signal_confirmation_bars,
            take_profit_pct: strategy.take_profit_pct,
            profit_target_multiplier: strategy.profit_target_multiplier,

            // Risk
            max_positions: risk.max_positions,
            max_position_size_pct: risk.max_position_size_pct,
            max_position_value_usd: risk.max_position_value_usd,
            risk_per_trade_percent: risk.risk_per_trade_percent,
            max_daily_loss_pct: risk.max_daily_loss_pct,
            max_drawdown_pct: risk.max_drawdown_pct,
            consecutive_loss_limit: risk.consecutive_loss_limit,
            pending_order_ttl_ms: risk.pending_order_ttl_ms,
            max_sector_exposure_pct: risk.max_sector_exposure_pct,
            sector_map: risk.sector_map,
            non_pdt_mode: risk.non_pdt_mode,
            max_orders_per_minute: risk.max_orders_per_minute,
            order_cooldown_seconds: risk.order_cooldown_seconds,
            min_hold_time_minutes: risk.min_hold_time_minutes,
            slippage_pct: risk.slippage_pct,
            commission_per_share: risk.commission_per_share,
            spread_bps: risk.spread_bps,
            min_profit_ratio: risk.min_profit_ratio,
            initial_cash: risk.initial_cash,
            trade_quantity: risk.trade_quantity,
            portfolio_staleness_ms: risk.portfolio_staleness_ms,
            portfolio_refresh_interval_ms: risk.portfolio_refresh_interval_ms,
            dynamic_symbol_mode: risk.dynamic_symbol_mode,
            dynamic_scan_interval_minutes: risk.dynamic_scan_interval_minutes,
            symbols: risk.symbols,
            min_volume_threshold: risk.min_volume_threshold,
            adaptive_optimization_enabled: risk.adaptive_optimization_enabled,
            regime_detection_window: risk.regime_detection_window,
            adaptive_evaluation_hour: risk.adaptive_evaluation_hour,
            risk_appetite: strategy.risk_appetite,

            // Observability
            observability_enabled: observability.enabled,
            observability_port: observability.port,
            observability_bind_address: observability.bind_address,
        })
    }

    pub fn create_fee_model(
        &self,
    ) -> std::sync::Arc<dyn crate::domain::trading::fee_model::FeeModel> {
        use crate::domain::trading::fee_model::{ConstantFeeModel, TieredFeeModel};
        use rust_decimal::prelude::FromPrimitive;

        match self.asset_class {
            AssetClass::Stock => std::sync::Arc::new(ConstantFeeModel::new(
                Decimal::from_f64(self.commission_per_share).unwrap_or(Decimal::ZERO),
                Decimal::from_f64(self.slippage_pct).unwrap_or(Decimal::ZERO),
            )),
            AssetClass::Crypto => std::sync::Arc::new(TieredFeeModel::new(
                Decimal::ZERO,
                Decimal::from_f64(self.commission_per_share).unwrap_or(Decimal::ZERO),
                Decimal::from_f64(self.slippage_pct).unwrap_or(Decimal::ZERO),
            )),
        }
    }

    /// Create a RiskConfig domain value object from this Config
    pub fn to_risk_config(&self) -> Result<crate::domain::config::RiskConfig> {
        crate::domain::config::RiskConfig::new(
            self.max_position_size_pct,
            self.max_sector_exposure_pct,
            self.max_daily_loss_pct,
            self.max_drawdown_pct,
            self.consecutive_loss_limit,
            self.pending_order_ttl_ms,
        )
        .map_err(|e| anyhow::anyhow!("Invalid risk config: {}", e))
    }

    /// Create a StrategyConfig domain value object from this Config
    pub fn to_strategy_config(&self) -> Result<crate::domain::config::StrategyConfig> {
        crate::domain::config::StrategyConfig::new(
            self.strategy_mode,
            self.fast_sma_period,
            self.slow_sma_period,
            self.trend_sma_period,
            self.rsi_period,
            self.rsi_threshold,
            self.macd_fast_period,
            self.macd_slow_period,
            self.macd_signal_period,
            self.macd_requires_rising,
            self.macd_min_threshold,
            self.adx_period,
            self.adx_threshold,
            self.trend_divergence_threshold,
            self.trend_tolerance_pct,
            self.signal_confirmation_bars,
            self.primary_timeframe,
            self.enabled_timeframes.clone(),
            self.trend_timeframe,
        )
        .map_err(|e| anyhow::anyhow!("Invalid strategy config: {}", e))
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
                self.alpaca_api_key.clone(),
                self.alpaca_secret_key.clone(),
                self.alpaca_base_url.clone(),
                self.alpaca_ws_url.clone(),
                Some(self.alpaca_data_url.clone()),
            ),
            Mode::Binance => (
                self.binance_api_key.clone(),
                self.binance_secret_key.clone(),
                self.binance_base_url.clone(),
                self.binance_ws_url.clone(),
                None,
            ),
            Mode::Oanda => (
                self.oanda_api_key.clone(),
                String::new(),
                self.oanda_api_base_url.clone(),
                self.oanda_stream_base_url.clone(),
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
        assert_eq!(config.max_positions, 5);
        assert_eq!(config.fast_sma_period, 20);
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
