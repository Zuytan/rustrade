use anyhow::{Context, Result};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::env;

#[derive(Debug, Clone)]
pub enum Mode {
    Mock,
    Alpaca,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyMode {
    Standard,
    Advanced,
    Dynamic,
    TrendRiding,
}

impl StrategyMode {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "standard" => Ok(StrategyMode::Standard),
            "advanced" => Ok(StrategyMode::Advanced),
            "dynamic" => Ok(StrategyMode::Dynamic),
            "trendriding" => Ok(StrategyMode::TrendRiding),
            _ => anyhow::bail!(
                "Invalid STRATEGY_MODE: {}. Must be 'standard', 'advanced', 'dynamic', or 'trendriding'",
                s
            ),
        }
    }
}

impl Mode {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "mock" => Ok(Mode::Mock),
            "alpaca" => Ok(Mode::Alpaca),
            _ => anyhow::bail!("Invalid MODE: {}. Must be 'mock' or 'alpaca'", s),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub alpaca_api_key: String,
    pub alpaca_secret_key: String,
    pub alpaca_base_url: String,
    pub alpaca_ws_url: String,
    pub symbols: Vec<String>,
    pub max_positions: usize,
    pub initial_cash: Decimal,
    pub fast_sma_period: usize,
    pub slow_sma_period: usize,
    pub max_orders_per_minute: u32,
    pub trade_quantity: Decimal,
    pub sma_threshold: f64,
    pub order_cooldown_seconds: u64,
    pub risk_per_trade_percent: f64,
    pub non_pdt_mode: bool,
    pub strategy_mode: StrategyMode,
    pub trend_sma_period: usize,
    pub rsi_period: usize,
    pub macd_fast_period: usize,
    pub macd_slow_period: usize,
    pub macd_signal_period: usize,
    pub trend_divergence_threshold: f64,
    pub rsi_threshold: f64,
    pub dynamic_symbol_mode: bool,
    pub dynamic_scan_interval_minutes: u64,
    pub trailing_stop_atr_multiplier: f64,
    pub atr_period: usize,
    // Risk Management
    pub max_position_size_pct: f64,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub consecutive_loss_limit: usize,
    // Transaction Costs
    pub slippage_pct: f64,
    pub commission_per_share: f64,
    // Trend Riding Strategy
    pub trend_riding_exit_buffer_pct: f64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let mode_str = env::var("MODE").unwrap_or_else(|_| "mock".to_string());
        let mode = Mode::from_str(&mode_str)?;

        let alpaca_api_key = env::var("ALPACA_API_KEY").unwrap_or_default();
        let alpaca_secret_key = env::var("ALPACA_SECRET_KEY").unwrap_or_default();
        let alpaca_base_url = env::var("ALPACA_BASE_URL")
            .unwrap_or_else(|_| "https://paper-api.alpaca.markets".to_string());
        let alpaca_ws_url = env::var("ALPACA_WS_URL")
            .unwrap_or_else(|_| "wss://stream.data.alpaca.markets/v2/iex".to_string());

        let symbols_str = env::var("SYMBOLS").unwrap_or_else(|_| "AAPL".to_string());
        let symbols: Vec<String> = symbols_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let max_positions = env::var("MAX_POSITIONS")
            .unwrap_or_else(|_| "5".to_string())
            .parse::<usize>()
            .context("Failed to parse MAX_POSITIONS")?;

        let initial_cash = env::var("INITIAL_CASH")
            .unwrap_or_else(|_| "100000.0".to_string())
            .parse::<f64>()
            .context("Failed to parse INITIAL_CASH")?;

        let fast_sma_period = env::var("FAST_SMA_PERIOD")
            .unwrap_or_else(|_| "20".to_string())
            .parse::<usize>()
            .context("Failed to parse FAST_SMA_PERIOD")?;

        let slow_sma_period = env::var("SLOW_SMA_PERIOD")
            .unwrap_or_else(|_| "60".to_string())
            .parse::<usize>()
            .context("Failed to parse SLOW_SMA_PERIOD")?;

        let max_orders_per_minute = env::var("MAX_ORDERS_PER_MINUTE")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u32>()
            .context("Failed to parse MAX_ORDERS_PER_MINUTE")?;

        let trade_quantity = env::var("TRADE_QUANTITY")
            .unwrap_or_else(|_| "1.0".to_string())
            .parse::<f64>()
            .context("Failed to parse TRADE_QUANTITY")?;

        let sma_threshold = env::var("SMA_THRESHOLD")
            .unwrap_or_else(|_| "0.001".to_string())
            .parse::<f64>()
            .context("Failed to parse SMA_THRESHOLD")?;

        let order_cooldown_seconds = env::var("ORDER_COOLDOWN_SECONDS")
            .unwrap_or_else(|_| "300".to_string())
            .parse::<u64>()
            .context("Failed to parse ORDER_COOLDOWN_SECONDS")?;

        let risk_per_trade_percent = env::var("RISK_PER_TRADE_PERCENT")
            .unwrap_or_else(|_| "0.02".to_string())
            .parse::<f64>()
            .context("Failed to parse RISK_PER_TRADE_PERCENT")?;

        let non_pdt_mode = env::var("NON_PDT_MODE")
            .unwrap_or_else(|_| "true".to_string())
            .parse::<bool>()
            .unwrap_or(true);

        let strategy_mode_str =
            env::var("STRATEGY_MODE").unwrap_or_else(|_| "standard".to_string());
        let strategy_mode = StrategyMode::from_str(&strategy_mode_str)?;

        let trend_sma_period = env::var("TREND_SMA_PERIOD")
            .unwrap_or_else(|_| "2000".to_string())
            .parse::<usize>()
            .context("Failed to parse TREND_SMA_PERIOD")?;

        let rsi_period = env::var("RSI_PERIOD")
            .unwrap_or_else(|_| "14".to_string())
            .parse::<usize>()
            .context("Failed to parse RSI_PERIOD")?;

        let macd_fast_period = env::var("MACD_FAST_PERIOD")
            .unwrap_or_else(|_| "12".to_string())
            .parse::<usize>()
            .context("Failed to parse MACD_FAST_PERIOD")?;

        let macd_slow_period = env::var("MACD_SLOW_PERIOD")
            .unwrap_or_else(|_| "26".to_string())
            .parse::<usize>()
            .context("Failed to parse MACD_SLOW_PERIOD")?;

        let macd_signal_period = env::var("MACD_SIGNAL_PERIOD")
            .unwrap_or_else(|_| "9".to_string())
            .parse::<usize>()
            .context("Failed to parse MACD_SIGNAL_PERIOD")?;

        let trend_divergence_threshold = env::var("TREND_DIVERGENCE_THRESHOLD")
            .unwrap_or_else(|_| "0.005".to_string())
            .parse::<f64>()
            .context("Failed to parse TREND_DIVERGENCE_THRESHOLD")?;

        let rsi_threshold = env::var("RSI_THRESHOLD")
            .unwrap_or_else(|_| "65.0".to_string())
            .parse::<f64>()
            .context("Failed to parse RSI_THRESHOLD")?;

        let dynamic_symbol_mode = env::var("DYNAMIC_SYMBOL_MODE")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);

        let dynamic_scan_interval_minutes = env::var("DYNAMIC_SCAN_INTERVAL_MINUTES")
            .unwrap_or_else(|_| "5".to_string())
            .parse::<u64>()
            .context("Failed to parse DYNAMIC_SCAN_INTERVAL_MINUTES")?;

        let trailing_stop_atr_multiplier = env::var("TRAILING_STOP_ATR_MULTIPLIER")
            .unwrap_or_else(|_| "3.0".to_string())
            .parse::<f64>()
            .context("Failed to parse TRAILING_STOP_ATR_MULTIPLIER")?;

        let atr_period = env::var("ATR_PERIOD")
            .unwrap_or_else(|_| "14".to_string())
            .parse::<usize>()
            .context("Failed to parse ATR_PERIOD")?;

        let max_position_size_pct = env::var("MAX_POSITION_SIZE_PCT")
            .unwrap_or_else(|_| "0.1".to_string())
            .parse::<f64>()
            .context("Failed to parse MAX_POSITION_SIZE_PCT")?;

        let max_daily_loss_pct = env::var("MAX_DAILY_LOSS_PCT")
            .unwrap_or_else(|_| "0.02".to_string())
            .parse::<f64>()
            .context("Failed to parse MAX_DAILY_LOSS_PCT")?;

        let max_drawdown_pct = env::var("MAX_DRAWDOWN_PCT")
            .unwrap_or_else(|_| "0.1".to_string())
            .parse::<f64>()
            .context("Failed to parse MAX_DRAWDOWN_PCT")?;

        let consecutive_loss_limit = env::var("CONSECUTIVE_LOSS_LIMIT")
            .unwrap_or_else(|_| "3".to_string())
            .parse::<usize>()
            .context("Failed to parse CONSECUTIVE_LOSS_LIMIT")?;

        let slippage_pct = env::var("SLIPPAGE_PCT")
            .unwrap_or_else(|_| "0.001".to_string())
            .parse::<f64>()
            .context("Failed to parse SLIPPAGE_PCT")?;

        let commission_per_share = env::var("COMMISSION_PER_SHARE")
            .unwrap_or_else(|_| "0.001".to_string())
            .parse::<f64>()
            .context("Failed to parse COMMISSION_PER_SHARE")?;

        let trend_riding_exit_buffer_pct = env::var("TREND_RIDING_EXIT_BUFFER_PCT")
            .unwrap_or_else(|_| "0.03".to_string())
            .parse::<f64>()
            .context("Failed to parse TREND_RIDING_EXIT_BUFFER_PCT")?;

        Ok(Config {
            mode,
            alpaca_api_key,
            alpaca_secret_key,
            alpaca_base_url,
            alpaca_ws_url,
            symbols,
            max_positions,
            initial_cash: Decimal::from_f64(initial_cash).unwrap_or(Decimal::from(100000)),
            fast_sma_period,
            slow_sma_period,
            max_orders_per_minute,
            trade_quantity: Decimal::from_f64(trade_quantity).unwrap_or(Decimal::from(1)),
            sma_threshold,
            order_cooldown_seconds,
            risk_per_trade_percent,
            non_pdt_mode,
            strategy_mode,
            trend_sma_period,
            rsi_period,
            macd_fast_period,
            macd_slow_period,
            macd_signal_period,
            trend_divergence_threshold,
            rsi_threshold,
            dynamic_symbol_mode,
            dynamic_scan_interval_minutes,
            trailing_stop_atr_multiplier,
            atr_period,
            max_position_size_pct,
            max_daily_loss_pct,
            max_drawdown_pct,
            consecutive_loss_limit,
            slippage_pct,
            commission_per_share,
            trend_riding_exit_buffer_pct,
        })
    }
}
