use std::sync::Arc;
use tokio::sync::RwLock;
use crate::config::{Config, Mode};
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::portfolio::Portfolio;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::infrastructure::alpaca::{AlpacaExecutionService, AlpacaMarketDataService};
use crate::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use crate::infrastructure::oanda::{OandaExecutionService, OandaMarketDataService};

pub struct ServiceFactory;

impl ServiceFactory {
    pub fn create_services(
        config: &Config,
        candle_repo: Option<Arc<dyn CandleRepository>>,
        portfolio: Arc<RwLock<Portfolio>>,
    ) -> (
        Arc<dyn MarketDataService>,
        Arc<dyn ExecutionService>,
        Arc<SpreadCache>,
    ) {
        match config.mode {
            Mode::Mock => {
                (
                    Arc::new(MockMarketDataService::new()),
                    Arc::new(MockExecutionService::new(portfolio)),
                    Arc::new(SpreadCache::new()),
                )
            }
            Mode::Alpaca => {
                let market_service = AlpacaMarketDataService::builder()
                    .api_key(config.alpaca_api_key.clone())
                    .api_secret(config.alpaca_secret_key.clone())
                    .ws_url(config.alpaca_ws_url.clone())
                    .data_base_url(config.alpaca_data_url.clone())
                    .min_volume_threshold(config.min_volume_threshold)
                    .asset_class(config.asset_class)
                    .candle_repository(candle_repo)
                    .build();

                let spread_cache = market_service.get_spread_cache();

                let execution_service = AlpacaExecutionService::new(
                    config.alpaca_api_key.clone(),
                    config.alpaca_secret_key.clone(),
                    config.alpaca_base_url.clone(),
                );

                (
                    Arc::new(market_service),
                    Arc::new(execution_service),
                    spread_cache,
                )
            }
            Mode::Oanda => {
                (
                    Arc::new(OandaMarketDataService::new(
                        config.oanda_api_key.clone(),
                        config.oanda_stream_base_url.clone(),
                        config.oanda_api_base_url.clone(),
                        config.oanda_account_id.clone(),
                    )),
                    Arc::new(OandaExecutionService::new(
                        config.oanda_api_key.clone(),
                        config.oanda_api_base_url.clone(),
                        config.oanda_account_id.clone(),
                    )),
                    Arc::new(SpreadCache::new()),
                )
            }
        }
    }
}
