pub use crate::domain::market::strategy_config::StrategyMode;
use crate::domain::risk::risk_appetite::RiskAppetite;
use anyhow::{Context, Result};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::env;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum Mode {
    Mock,
    Alpaca,
    Oanda,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetClass {
    Stock,
    Crypto,
}

impl std::str::FromStr for AssetClass {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stock" => Ok(AssetClass::Stock),
            "crypto" => Ok(AssetClass::Crypto),
            _ => anyhow::bail!("Invalid ASSET_CLASS: {}. Must be 'stock' or 'crypto'", s),
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mock" => Ok(Mode::Mock),
            "alpaca" => Ok(Mode::Alpaca),
            "oanda" => Ok(Mode::Oanda),
            _ => anyhow::bail!("Invalid MODE: {}. Must be 'mock', 'alpaca' or 'oanda'", s),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub asset_class: AssetClass,
    pub alpaca_api_key: String,
    pub alpaca_secret_key: String,
    pub alpaca_base_url: String,
    pub alpaca_data_url: String, // Added for Data API
    pub alpaca_ws_url: String,
    // OANDA Config
    pub oanda_api_base_url: String,
    pub oanda_stream_base_url: String,
    pub oanda_api_key: String,
    pub oanda_account_id: String,
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
    pub max_sector_exposure_pct: f64, // Added
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub consecutive_loss_limit: usize,
    // Transaction Costs
    pub slippage_pct: f64,
    pub commission_per_share: f64,
    pub spread_bps: f64,              // New: spread in basis points (e.g., 5.0 = 5 bps)
    pub min_profit_ratio: f64,        // New: minimum profit/cost ratio (e.g., 2.0 = 2x)
    pub trend_riding_exit_buffer_pct: f64,
    pub mean_reversion_rsi_exit: f64,
    pub mean_reversion_bb_period: usize,
    pub risk_appetite: Option<RiskAppetite>,
    // Metadata
    pub sector_map: std::collections::HashMap<String, String>, // Added
    pub portfolio_staleness_ms: u64,
    pub portfolio_refresh_interval_ms: u64,  // New: interval for periodic portfolio refresh
    // Adaptive Optimization
    pub adaptive_optimization_enabled: bool,
    pub regime_detection_window: usize,
    pub adaptive_evaluation_hour: u32,
    pub min_volume_threshold: f64, // Added for symbol screening
    pub max_position_value_usd: f64,  // New: cap on position value in USD
    pub signal_confirmation_bars: usize,  // Phase 2: require N bars of confirmation
    pub min_hold_time_minutes: i64,       // Phase 2: minimum hold time in minutes
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,
    pub take_profit_pct: f64,
    // Risk-based adaptive filters
    pub macd_requires_rising: bool,
    pub trend_tolerance_pct: f64,
    pub macd_min_threshold: f64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let mode_str = env::var("MODE").unwrap_or_else(|_| "mock".to_string());
        let mode = Mode::from_str(&mode_str)?;

        let asset_class_str = env::var("ASSET_CLASS").unwrap_or_else(|_| "stock".to_string());
        let asset_class = AssetClass::from_str(&asset_class_str)?;

        let alpaca_api_key = env::var("ALPACA_API_KEY").unwrap_or_default();
        let alpaca_secret_key = env::var("ALPACA_SECRET_KEY").unwrap_or_default();
        let alpaca_base_url = env::var("ALPACA_BASE_URL")
            .unwrap_or_else(|_| "https://paper-api.alpaca.markets".to_string());
        let alpaca_data_url = env::var("ALPACA_DATA_URL")
            .unwrap_or_else(|_| "https://data.alpaca.markets".to_string());
        let alpaca_ws_url = env::var("ALPACA_WS_URL")
            .unwrap_or_else(|_| "wss://stream.data.alpaca.markets/v2/iex".to_string());

        let oanda_api_base_url = env::var("OANDA_API_BASE_URL")
            .unwrap_or_else(|_| "https://api-fxpractice.oanda.com".to_string());
        let oanda_stream_base_url = env::var("OANDA_STREAM_BASE_URL")
            .unwrap_or_else(|_| "https://stream-fxpractice.oanda.com".to_string());
        let oanda_api_key = env::var("OANDA_API_KEY").unwrap_or_default();
        let oanda_account_id = env::var("OANDA_ACCOUNT_ID").unwrap_or_default();

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
            .unwrap_or_else(|_| "0.015".to_string())  // Reduced from 0.02 to 1.5%
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
            .unwrap_or_else(|_| "75.0".to_string())
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
            .unwrap_or_else(|_| "5.0".to_string())  // Increased from 4.0 to 5.0
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

        let max_position_value_usd = env::var("MAX_POSITION_VALUE_USD")
            .unwrap_or_else(|_| "5000.0".to_string())
            .parse::<f64>()
            .context("Failed to parse MAX_POSITION_VALUE_USD")?;

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

        let mean_reversion_rsi_exit = env::var("MEAN_REVERSION_RSI_EXIT")
            .unwrap_or_else(|_| "50.0".to_string())
            .parse::<f64>()
            .context("Failed to parse MEAN_REVERSION_RSI_EXIT")?;

        let mean_reversion_bb_period = env::var("MEAN_REVERSION_BB_PERIOD")
            .unwrap_or_else(|_| "20".to_string())
            .parse::<usize>()
            .context("Failed to parse MEAN_REVERSION_BB_PERIOD")?;

        // Sector Config
        let sectors_env = env::var("SECTORS").unwrap_or_default();
        let mut sector_map = std::collections::HashMap::new();
        for entry in sectors_env.split(',') {
            if let Some((sym, sec)) = entry.split_once(':') {
                sector_map.insert(sym.trim().to_string(), sec.trim().to_string());
            }
        }

        let max_sector_exposure_pct = env::var("MAX_SECTOR_EXPOSURE_PCT")
            .unwrap_or_else(|_| "0.30".to_string())
            .parse::<f64>()
            .context("Failed to parse MAX_SECTOR_EXPOSURE_PCT")?;

        // Phase 2: Strategy Refinement Parameters
        let signal_confirmation_bars = env::var("SIGNAL_CONFIRMATION_BARS")
            .unwrap_or_else(|_| "2".to_string())
            .parse::<usize>()
            .context("Failed to parse SIGNAL_CONFIRMATION_BARS")?;

        let min_hold_time_minutes = env::var("MIN_HOLD_TIME_MINUTES")
            .unwrap_or_else(|_| "240".to_string())  // 4 hours default
            .parse::<i64>()
            .context("Failed to parse MIN_HOLD_TIME_MINUTES")?;

        let spread_bps = env::var("SPREAD_BPS")
            .unwrap_or_else(|_| "5.0".to_string())
            .parse::<f64>()
            .unwrap_or(5.0);

        let min_profit_ratio = env::var("MIN_PROFIT_RATIO")
            .unwrap_or_else(|_| "2.0".to_string())
            .parse::<f64>()
            .unwrap_or(2.0);

        // Risk Appetite Score (optional - overrides individual risk params if set)
        let risk_appetite = if let Ok(score_str) = env::var("RISK_APPETITE_SCORE") {
            let score = score_str
                .parse::<u8>()
                .context("Failed to parse RISK_APPETITE_SCORE - must be integer 1-9")?;
            Some(RiskAppetite::new(score).context("RISK_APPETITE_SCORE must be between 1 and 9")?)
        } else {
            None
        };

        // If risk_appetite is set, override the individual risk parameters
        let (
            final_risk_per_trade,
            final_trailing_stop,
            final_rsi_threshold,
            final_max_position_size,
            final_min_profit_ratio,
            final_macd_requires_rising,
            final_trend_tolerance_pct,
            final_macd_min_threshold,
        ) = if let Some(ref appetite) = risk_appetite {
            (
                appetite.calculate_risk_per_trade_percent(),
                appetite.calculate_trailing_stop_multiplier(),
                appetite.calculate_rsi_threshold(),
                appetite.calculate_max_position_size_pct(),
                appetite.calculate_min_profit_ratio(),
                appetite.requires_macd_rising(),
                appetite.calculate_trend_tolerance_pct(),
                appetite.calculate_macd_min_threshold(),
            )
        } else {
            // Use individual env vars as before (backward compatibility)
            (
                risk_per_trade_percent,
                trailing_stop_atr_multiplier,
                rsi_threshold,
                max_position_size_pct,
                min_profit_ratio,
                true,  // Conservative default: require MACD rising
                0.0,   // Conservative default: strict trend alignment  
                0.0,   // Conservative default: neutral MACD threshold
            )
        };

        let adaptive_optimization_enabled = env::var("ADAPTIVE_OPTIMIZATION_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);
        let regime_detection_window = env::var("REGIME_DETECTION_WINDOW")
            .unwrap_or_else(|_| "20".to_string())
            .parse::<usize>()
            .unwrap_or(20);
        let adaptive_evaluation_hour = env::var("ADAPTIVE_EVALUATION_HOUR")
            .unwrap_or_else(|_| "0".to_string())
            .parse::<u32>()
            .unwrap_or(0);

        let min_volume_threshold = env::var("MIN_VOLUME_THRESHOLD")
            .unwrap_or_else(|_| "50000.0".to_string())
            .parse::<f64>()
            .unwrap_or(50000.0);

        let ema_fast_period = env::var("EMA_FAST_PERIOD")
            .unwrap_or_else(|_| "50".to_string())
            .parse::<usize>()
            .unwrap_or(50);

        let ema_slow_period = env::var("EMA_SLOW_PERIOD")
            .unwrap_or_else(|_| "150".to_string())
            .parse::<usize>()
            .unwrap_or(150);

        let take_profit_pct = env::var("TAKE_PROFIT_PCT")
            .unwrap_or_else(|_| "0.05".to_string())
            .parse::<f64>()
            .unwrap_or(0.05);

        let portfolio_staleness_ms = env::var("PORTFOLIO_STALENESS_MS")
            .unwrap_or_else(|_| "5000".to_string())
            .parse::<u64>()
            .unwrap_or(5000);

        let portfolio_refresh_interval_ms = env::var("PORTFOLIO_REFRESH_INTERVAL_MS")
            .unwrap_or_else(|_| "2000".to_string())
            .parse::<u64>()
            .unwrap_or(2000);

        Ok(Config {
            mode,
            asset_class,
            alpaca_api_key,
            alpaca_secret_key,
            alpaca_base_url,
            alpaca_data_url,
            alpaca_ws_url,
            oanda_api_base_url,
            oanda_stream_base_url,
            oanda_api_key,
            oanda_account_id,
            symbols,
            max_positions,
            initial_cash: Decimal::from_f64(initial_cash).unwrap_or(Decimal::from(100000)),
            fast_sma_period,
            slow_sma_period,
            max_orders_per_minute,
            trade_quantity: Decimal::from_f64(trade_quantity).unwrap_or(Decimal::from(1)),
            sma_threshold,
            order_cooldown_seconds,
            risk_per_trade_percent: final_risk_per_trade,
            non_pdt_mode,
            strategy_mode,
            trend_sma_period,
            rsi_period,
            macd_fast_period,
            macd_slow_period,
            macd_signal_period,
            trend_divergence_threshold,
            rsi_threshold: final_rsi_threshold,
            dynamic_symbol_mode,
            dynamic_scan_interval_minutes,
            trailing_stop_atr_multiplier: final_trailing_stop,
            atr_period,
            max_position_size_pct: final_max_position_size,
            max_position_value_usd,
            max_daily_loss_pct,
            max_drawdown_pct,
            consecutive_loss_limit,
            slippage_pct,
            commission_per_share,
            trend_riding_exit_buffer_pct,
            mean_reversion_rsi_exit,
            mean_reversion_bb_period,
            risk_appetite,
            max_sector_exposure_pct,
            sector_map,
            adaptive_optimization_enabled,
            regime_detection_window,
            adaptive_evaluation_hour,
            min_volume_threshold,
            signal_confirmation_bars,
            min_hold_time_minutes,
            ema_fast_period,
            ema_slow_period,
            take_profit_pct,
            macd_requires_rising: final_macd_requires_rising,
            trend_tolerance_pct: final_trend_tolerance_pct,
            macd_min_threshold: final_macd_min_threshold,
            spread_bps,
            min_profit_ratio: final_min_profit_ratio,
            portfolio_staleness_ms,
            portfolio_refresh_interval_ms,
        })
    }
}
