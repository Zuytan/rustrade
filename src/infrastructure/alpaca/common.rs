use serde::{Deserialize, Serialize};

// ===== Constants =====

/// Major crypto pairs to scan for top movers
/// Since Alpaca doesn't provide a movers API for crypto, we maintain a curated list
pub const CRYPTO_UNIVERSE: &[&str] = &[
    "BTC/USD",
    "ETH/USD",
    "AVAX/USD",
    "SOL/USD",
    "MATIC/USD",
    "LINK/USD",
    "UNI/USD",
    "AAVE/USD",
    "DOT/USD",
    "ATOM/USD",
];

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct AlpacaBar {
    #[serde(rename = "t")]
    pub timestamp: String,
    #[serde(rename = "o")]
    pub open: f64,
    #[serde(rename = "h")]
    pub high: f64,
    #[serde(rename = "l")]
    pub low: f64,
    #[serde(rename = "c")]
    pub close: f64,
    #[serde(rename = "v")]
    pub volume: f64,
}
