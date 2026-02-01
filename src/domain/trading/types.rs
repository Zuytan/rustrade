use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl fmt::Display for OrderSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "BUY"),
            OrderSide::Sell => write!(f, "SELL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    DoneForDay,
    Canceled,
    Cancelled, // Alias for Canceled
    Replaced,
    PendingCancel,
    Stopped,
    Rejected,
    Suspended,
    PendingNew,
    Calculated,
    Expired,
    Accepted,
    PendingReplace,
    Pending, // Added to match usage
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Candle {
    pub symbol: String,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketEvent {
    Quote {
        symbol: String,
        price: Decimal,
        quantity: Decimal,
        timestamp: i64,
    },
    Candle(Candle),
    SymbolSubscription {
        symbol: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderType::Market => write!(f, "MARKET"),
            OrderType::Limit => write!(f, "LIMIT"),
            OrderType::Stop => write!(f, "STOP"),
            OrderType::StopLimit => write!(f, "STOP_LIMIT"),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TradeProposal {
    pub symbol: String,
    pub side: OrderSide,
    pub price: Decimal,
    pub quantity: Decimal,
    pub order_type: OrderType,
    pub reason: String,
    pub timestamp: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Order {
    pub id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub price: Decimal,
    pub quantity: Decimal,
    pub order_type: OrderType,
    pub status: OrderStatus,
    pub timestamp: i64,
}

/// Represents a completed trade with profit/loss information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub entry_price: Decimal,
    pub exit_price: Option<Decimal>,
    pub quantity: Decimal,
    pub pnl: Decimal, // Realized profit/loss
    pub entry_timestamp: i64,
    pub exit_timestamp: Option<i64>,
}

impl Trade {
    /// Create a new trade from an opening order
    pub fn from_order(order: &Order) -> Self {
        Self {
            id: order.id.clone(),
            symbol: order.symbol.clone(),
            side: order.side,
            entry_price: order.price,
            exit_price: None,
            quantity: order.quantity,
            pnl: Decimal::ZERO,
            entry_timestamp: order.timestamp,
            exit_timestamp: None,
        }
    }

    /// Close the trade and calculate P&L
    pub fn close(&mut self, exit_price: Decimal, exit_timestamp: i64) {
        self.exit_price = Some(exit_price);
        self.exit_timestamp = Some(exit_timestamp);

        // Calculate P&L: (exit - entry) * quantity for buy, (entry - exit) * quantity for sell
        self.pnl = match self.side {
            OrderSide::Buy => (exit_price - self.entry_price) * self.quantity,
            OrderSide::Sell => (self.entry_price - exit_price) * self.quantity,
        };
    }
}

/// Lifecycle state of a position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionLifecycle {
    /// Position has been opened
    Opened,
    /// Position is active and being monitored
    Active,
    /// Position has been closed
    Closed,
}

/// Technical indicators for a symbol
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeatureSet {
    pub last_price: Option<Decimal>,
    pub rsi: Option<Decimal>,
    pub macd_line: Option<Decimal>,
    pub macd_signal: Option<Decimal>,
    pub macd_hist: Option<Decimal>,
    pub sma_20: Option<Decimal>,
    pub sma_50: Option<Decimal>,
    pub sma_200: Option<Decimal>,
    pub bb_upper: Option<Decimal>,
    pub bb_middle: Option<Decimal>,
    pub bb_lower: Option<Decimal>,
    pub atr: Option<Decimal>,
    pub ema_fast: Option<Decimal>,
    pub ema_slow: Option<Decimal>,
    pub adx: Option<Decimal>,
    pub bb_width: Option<Decimal>,
    pub bb_position: Option<Decimal>,
    pub atr_pct: Option<Decimal>,

    // Advanced Statistical Features (Phase 2)
    pub hurst_exponent: Option<Decimal>,
    pub skewness: Option<Decimal>,
    pub momentum_normalized: Option<Decimal>,
    pub realized_volatility: Option<Decimal>,
    // Microstructure Features (Phase 2)
    pub ofi: Option<Decimal>,
    pub cumulative_delta: Option<Decimal>,
    pub spread_bps: Option<Decimal>,
    /// The timeframe these indicators were calculated on
    pub timeframe: Option<crate::domain::market::timeframe::Timeframe>,
}

// ===== Symbol Normalization =====

/// Supported quote currencies for crypto pairs, ordered by priority (longest first to prefer USDT over USD)
const CRYPTO_QUOTE_CURRENCIES: &[&str] = &[
    "USDT", "USDC", "BUSD", "TUSD", // Stablecoins (4 chars)
    "USD", "EUR", "GBP", "BTC", "ETH", // Traditional (3 chars)
];

/// Normalizes a crypto symbol from Alpaca format to application format.
///
/// Alpaca returns crypto symbols without slashes (e.g., "BTCUSD", "ETHUSDT"),
/// but the application uses slash-separated format (e.g., "BTC/USD", "ETH/USDT").
///
/// # Arguments
/// * `symbol` - The symbol to normalize (e.g., "BTCUSD", "ETHUSDT", "BTC/USD")
///
/// # Returns
/// * `Ok(String)` - The normalized symbol in "BASE/QUOTE" format
/// * `Err(String)` - Error message if the symbol cannot be normalized
///
/// # Examples
/// ```
/// use rustrade::domain::trading::types::normalize_crypto_symbol;
///
/// assert_eq!(normalize_crypto_symbol("BTCUSD").unwrap(), "BTC/USD");
/// assert_eq!(normalize_crypto_symbol("BTCUSDT").unwrap(), "BTC/USDT");
/// assert_eq!(normalize_crypto_symbol("ETHEUR").unwrap(), "ETH/EUR");
/// assert_eq!(normalize_crypto_symbol("BTC/USD").unwrap(), "BTC/USD"); // Already normalized
/// ```
pub fn normalize_crypto_symbol(symbol: &str) -> Result<String, String> {
    // Already normalized
    if symbol.contains('/') {
        return Ok(symbol.to_string());
    }

    // Empty check
    if symbol.is_empty() {
        return Err("Cannot normalize empty symbol".to_string());
    }

    // Try to match known quote currencies (longest first to prefer USDT over USD)
    for quote in CRYPTO_QUOTE_CURRENCIES {
        if symbol.ends_with(quote) && symbol.len() > quote.len() {
            let base = &symbol[..symbol.len() - quote.len()];
            // Validate base currency is not empty and looks reasonable (ASCII uppercase)
            if !base.is_empty() && base.chars().all(|c| c.is_ascii_uppercase()) {
                return Ok(format!("{}/{}", base, quote));
            }
        }
    }

    Err(format!(
        "Cannot normalize crypto symbol: '{}' - no recognized quote currency",
        symbol
    ))
}

/// Denormalizes a crypto symbol from application format back to Alpaca format.
///
/// This is the reverse of `normalize_crypto_symbol`, used when making API calls to Alpaca
/// that require symbols without slashes (e.g., snapshot API, historical bars API).
///
/// # Arguments
/// * `symbol` - The symbol to denormalize (e.g., "BTC/USD", "ETH/USDT")
///
/// # Returns
/// * `String` - The denormalized symbol in "BASEQUOTE" format (e.g., "BTCUSD", "ETHUSDT")
///
/// # Examples
/// ```
/// use rustrade::domain::trading::types::denormalize_crypto_symbol;
///
/// assert_eq!(denormalize_crypto_symbol("BTC/USD"), "BTCUSD");
/// assert_eq!(denormalize_crypto_symbol("ETH/USDT"), "ETHUSDT");
/// assert_eq!(denormalize_crypto_symbol("BTCUSD"), "BTCUSD"); // Already denormalized
/// ```
pub fn denormalize_crypto_symbol(symbol: &str) -> String {
    // Remove the slash if present
    symbol.replace('/', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_denormalize_crypto_symbol() {
        assert_eq!(denormalize_crypto_symbol("BTC/USD"), "BTCUSD");
        assert_eq!(denormalize_crypto_symbol("ETH/USDT"), "ETHUSDT");
        assert_eq!(denormalize_crypto_symbol("AVAX/USD"), "AVAXUSD");
        assert_eq!(denormalize_crypto_symbol("LINK/EUR"), "LINKEUR");

        // Already denormalized (no slash)
        assert_eq!(denormalize_crypto_symbol("BTCUSD"), "BTCUSD");
        assert_eq!(denormalize_crypto_symbol("ETHUSDT"), "ETHUSDT");
    }

    #[test]
    fn test_normalize_denormalize_roundtrip() {
        let symbols = vec!["BTCUSD", "ETHUSDT", "AVAXUSD", "LINKEUR"];

        for symbol in symbols {
            let normalized = normalize_crypto_symbol(symbol).unwrap();
            let denormalized = denormalize_crypto_symbol(&normalized);
            assert_eq!(denormalized, symbol);
        }
    }

    #[test]
    fn test_normalize_crypto_standard_pairs() {
        assert_eq!(normalize_crypto_symbol("BTCUSD").unwrap(), "BTC/USD");
        assert_eq!(normalize_crypto_symbol("ETHEUR").unwrap(), "ETH/EUR");
        assert_eq!(normalize_crypto_symbol("LTCGBP").unwrap(), "LTC/GBP");
        assert_eq!(normalize_crypto_symbol("BTCBTC").unwrap(), "BTC/BTC"); // Edge case but valid
        assert_eq!(normalize_crypto_symbol("LINKETH").unwrap(), "LINK/ETH");
    }

    #[test]
    fn test_normalize_crypto_stablecoins() {
        assert_eq!(normalize_crypto_symbol("BTCUSDT").unwrap(), "BTC/USDT");
        assert_eq!(normalize_crypto_symbol("ETHUSDC").unwrap(), "ETH/USDC");
        assert_eq!(normalize_crypto_symbol("BNBBUSD").unwrap(), "BNB/BUSD");
        assert_eq!(normalize_crypto_symbol("ADATUSD").unwrap(), "ADA/TUSD");
    }

    #[test]
    fn test_normalize_crypto_already_normalized() {
        assert_eq!(normalize_crypto_symbol("BTC/USD").unwrap(), "BTC/USD");
        assert_eq!(normalize_crypto_symbol("ETH/USDT").unwrap(), "ETH/USDT");
        assert_eq!(normalize_crypto_symbol("LINK/EUR").unwrap(), "LINK/EUR");
    }

    #[test]
    fn test_normalize_crypto_prefers_longer_quote() {
        // Should prefer USDT (4 chars) over USD (3 chars)
        assert_eq!(normalize_crypto_symbol("BTCUSDT").unwrap(), "BTC/USDT");
        // Not BTCU/SDT or BTC/USD
    }

    #[test]
    fn test_normalize_crypto_invalid_symbols() {
        // No recognized quote currency
        assert!(normalize_crypto_symbol("INVALID").is_err());
        assert!(normalize_crypto_symbol("ABC").is_err());
        assert!(normalize_crypto_symbol("GOOGLE").is_err());

        // Empty symbol
        assert!(normalize_crypto_symbol("").is_err());
    }

    #[test]
    fn test_normalize_crypto_edge_cases() {
        // Too short to have valid base after extracting quote
        assert!(normalize_crypto_symbol("USD").is_err());
        assert!(normalize_crypto_symbol("EUR").is_err());
        assert!(normalize_crypto_symbol("USDT").is_err());

        // Base would be empty
        let result = normalize_crypto_symbol("USD");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_crypto_case_sensitivity() {
        // Lowercase should fail since crypto symbols are uppercase
        assert!(normalize_crypto_symbol("btcusd").is_err());
        assert!(normalize_crypto_symbol("BtcUsd").is_err());
    }
}
