use crate::config::AssetClass;
use crate::domain::ports::MarketDataService;
use crate::domain::trading::types::MarketEvent;
use crate::infrastructure::alpaca::common::AlpacaBar;
use crate::infrastructure::alpaca::websocket::AlpacaWebSocketManager;
use crate::infrastructure::core::circuit_breaker::CircuitBreaker;
use crate::infrastructure::core::http_client_factory::HttpClientFactory;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

mod crypto_movers;
mod historical;
mod price;
mod response_parser;
mod sector_provider;
mod subscription;

pub use sector_provider::AlpacaSectorProvider;

pub struct AlpacaMarketDataService {
    pub(crate) client: ClientWithMiddleware,
    pub(crate) api_key: String,
    pub(crate) api_secret: String,
    pub(crate) ws_manager: Arc<AlpacaWebSocketManager>,
    pub(crate) data_base_url: String,
    pub(crate) api_base_url: String,
    pub(crate) bar_cache: std::sync::RwLock<std::collections::HashMap<String, Vec<AlpacaBar>>>,
    pub(crate) min_volume_threshold: f64,
    pub(crate) asset_class: AssetClass,
    pub(crate) spread_cache: Arc<crate::application::market_data::spread_cache::SpreadCache>,
    pub(crate) candle_repository: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
    pub(crate) circuit_breaker: Arc<CircuitBreaker>,
    /// Cache for tradable crypto assets (symbol list + timestamp)
    pub(crate) assets_cache: std::sync::RwLock<Option<(Vec<String>, std::time::Instant)>>,
}

impl AlpacaMarketDataService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api_key: String,
        api_secret: String,
        ws_url: String,
        data_base_url: String,
        api_base_url: String,
        min_volume_threshold: f64,
        asset_class: AssetClass,
        candle_repository: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
    ) -> Self {
        Self::builder()
            .api_key(api_key)
            .api_secret(api_secret)
            .ws_url(ws_url)
            .data_base_url(data_base_url)
            .api_base_url(api_base_url)
            .min_volume_threshold(min_volume_threshold)
            .asset_class(asset_class)
            .candle_repository(candle_repository)
            .build()
    }

    pub fn builder() -> AlpacaMarketDataServiceBuilder {
        AlpacaMarketDataServiceBuilder::default()
    }

    pub fn get_spread_cache(
        &self,
    ) -> Arc<crate::application::market_data::spread_cache::SpreadCache> {
        self.spread_cache.clone()
    }
}

#[derive(Default)]
pub struct AlpacaMarketDataServiceBuilder {
    api_key: Option<String>,
    api_secret: Option<String>,
    ws_url: Option<String>,
    data_base_url: Option<String>,
    api_base_url: Option<String>,
    min_volume_threshold: Option<f64>,
    asset_class: Option<AssetClass>,
    candle_repository: Option<Option<Arc<dyn crate::domain::repositories::CandleRepository>>>,
}

impl AlpacaMarketDataServiceBuilder {
    pub fn api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn api_secret(mut self, api_secret: String) -> Self {
        self.api_secret = Some(api_secret);
        self
    }

    pub fn ws_url(mut self, ws_url: String) -> Self {
        self.ws_url = Some(ws_url);
        self
    }

    pub fn data_base_url(mut self, data_base_url: String) -> Self {
        self.data_base_url = Some(data_base_url);
        self
    }

    pub fn api_base_url(mut self, api_base_url: String) -> Self {
        self.api_base_url = Some(api_base_url);
        self
    }

    pub fn min_volume_threshold(mut self, threshold: f64) -> Self {
        self.min_volume_threshold = Some(threshold);
        self
    }

    pub fn asset_class(mut self, asset_class: AssetClass) -> Self {
        self.asset_class = Some(asset_class);
        self
    }

    pub fn candle_repository(
        mut self,
        repo: Option<Arc<dyn crate::domain::repositories::CandleRepository>>,
    ) -> Self {
        self.candle_repository = Some(repo);
        self
    }

    pub fn build(self) -> AlpacaMarketDataService {
        let api_key = self.api_key.expect("api_key is required");
        let api_secret = self.api_secret.expect("api_secret is required");
        let ws_url = self.ws_url.expect("ws_url is required");
        let data_base_url = self.data_base_url.expect("data_base_url is required");
        let api_base_url = self.api_base_url.expect("api_base_url is required");
        let min_volume_threshold = self.min_volume_threshold.unwrap_or(100000.0);
        let asset_class = self.asset_class.unwrap_or(AssetClass::Stock);
        let candle_repository = self.candle_repository.flatten();

        let client = HttpClientFactory::create_client();
        let spread_cache =
            Arc::new(crate::application::market_data::spread_cache::SpreadCache::new());
        let ws_manager = Arc::new(AlpacaWebSocketManager::new(
            api_key.clone(),
            api_secret.clone(),
            ws_url,
            spread_cache.clone(),
        ));

        let circuit_breaker = Arc::new(CircuitBreaker::new(
            "AlpacaMarketData",
            5,
            3,
            std::time::Duration::from_secs(60),
        ));

        AlpacaMarketDataService {
            client,
            api_key,
            api_secret,
            ws_manager,
            data_base_url,
            api_base_url,
            bar_cache: std::sync::RwLock::new(std::collections::HashMap::new()),
            min_volume_threshold,
            asset_class,
            spread_cache,
            candle_repository,
            circuit_breaker,
            assets_cache: std::sync::RwLock::new(None),
        }
    }
}

#[async_trait]
impl MarketDataService for AlpacaMarketDataService {
    async fn subscribe(&self, symbols: Vec<String>) -> Result<Receiver<MarketEvent>> {
        self.subscribe_internal(symbols).await
    }

    async fn get_top_movers(&self) -> Result<Vec<String>> {
        self.get_top_movers_internal().await
    }

    async fn get_tradable_assets(&self) -> Result<Vec<String>> {
        self.get_tradable_assets_internal().await
    }

    async fn get_prices(
        &self,
        symbols: Vec<String>,
    ) -> Result<std::collections::HashMap<String, Decimal>> {
        self.get_prices_internal(symbols).await
    }

    async fn get_historical_bars(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
    ) -> Result<Vec<crate::domain::trading::types::Candle>> {
        self.get_historical_bars_internal(symbol, start, end, timeframe)
            .await
    }
}
