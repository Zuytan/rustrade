//! OANDA API client helpers and sector provider.

use crate::domain::ports::SectorProvider;
use anyhow::Result;
use async_trait::async_trait;

pub struct OandaSectorProvider;

#[async_trait]
impl SectorProvider for OandaSectorProvider {
    async fn get_sector(&self, _symbol: &str) -> Result<String> {
        Ok("Forex".to_string())
    }
}
