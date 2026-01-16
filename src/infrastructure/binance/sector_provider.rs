//! Binance Sector Provider
//!
//! Maps crypto symbols to their category/sector classification

use crate::domain::ports::SectorProvider;
use anyhow::Result;
use async_trait::async_trait;

pub struct BinanceSectorProvider;

#[async_trait]
impl SectorProvider for BinanceSectorProvider {
    async fn get_sector(&self, symbol: &str) -> Result<String> {
        // Map crypto symbols to categories
        let sector = if symbol.starts_with("BTC") || symbol.starts_with("ETH") {
            "Layer1"
        } else if symbol.starts_with("UNI")
            || symbol.starts_with("AAVE")
            || symbol.starts_with("LINK")
        {
            "DeFi"
        } else if symbol.starts_with("SOL")
            || symbol.starts_with("AVAX")
            || symbol.starts_with("DOT")
        {
            "Layer1"
        } else if symbol.starts_with("MATIC") {
            "Layer2"
        } else if symbol.starts_with("USDT") || symbol.starts_with("USDC") {
            "Stablecoin"
        } else {
            "Other"
        };

        Ok(sector.to_string())
    }
}
