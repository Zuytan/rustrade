use crate::infrastructure::core::http_client_factory::HttpClientFactory;
use anyhow::Result;
use async_trait::async_trait;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AlpacaAsset {
    #[serde(default)]
    sector: String,
}

pub struct AlpacaSectorProvider {
    client: ClientWithMiddleware,
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl AlpacaSectorProvider {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Self {
        Self {
            client: HttpClientFactory::create_client(),
            api_key,
            api_secret,
            base_url,
        }
    }
}

#[async_trait]
impl crate::domain::ports::SectorProvider for AlpacaSectorProvider {
    async fn get_sector(&self, symbol: &str) -> Result<String> {
        let url = format!("{}/v2/assets/{}", self.base_url, symbol);

        let response = self
            .client
            .get(&url)
            .header("APCA-API-KEY-ID", &self.api_key)
            .header("APCA-API-SECRET-KEY", &self.api_secret)
            .send()
            .await?;

        if response.status().is_success() {
            let asset: AlpacaAsset = response.json().await?;
            if asset.sector.is_empty() {
                Ok("Unknown".to_string())
            } else {
                Ok(asset.sector)
            }
        } else {
            Ok("Unknown".to_string())
        }
    }
}
