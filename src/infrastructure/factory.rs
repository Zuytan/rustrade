use crate::application::market_data::spread_cache::SpreadCache;
use crate::config::{Config, Mode};
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::portfolio::Portfolio;
use crate::infrastructure::alpaca::{AlpacaExecutionService, AlpacaMarketDataService};
use crate::infrastructure::binance::{BinanceExecutionService, BinanceMarketDataService};
use crate::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use crate::infrastructure::oanda::{OandaExecutionService, OandaMarketDataService};
use crate::infrastructure::observability::Metrics;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ServiceFactory;

impl ServiceFactory {
    pub fn create_services(
        config: &Config,
        candle_repo: Option<Arc<dyn CandleRepository>>,
        portfolio: Arc<RwLock<Portfolio>>,
        metrics: Metrics,
    ) -> (
        Arc<dyn MarketDataService>,
        Arc<dyn ExecutionService>,
        Arc<SpreadCache>,
    ) {
        match config.mode {
            Mode::Mock => {
                let execution_service = if config.simulation_enabled {
                    use crate::infrastructure::simulation::latency_model::NetworkLatency;
                    use crate::infrastructure::simulation::slippage_model::VolatilitySlippage;

                    let latency_model = Arc::new(NetworkLatency::new(
                        config.simulation_latency_base_ms,
                        config.simulation_latency_jitter_ms,
                    ));
                    let slippage_model = Arc::new(VolatilitySlippage::new(
                        config.simulation_slippage_volatility,
                    ));

                    MockExecutionService::with_simulation_models(
                        portfolio,
                        config.create_fee_model(),
                        latency_model,
                        slippage_model,
                    )
                } else {
                    MockExecutionService::with_costs(portfolio, config.create_fee_model())
                };

                (
                    Arc::new(MockMarketDataService::new()),
                    Arc::new(execution_service),
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
                    portfolio.clone(),
                    metrics.clone(),
                );

                (
                    Arc::new(market_service),
                    Arc::new(execution_service),
                    spread_cache,
                )
            }
            Mode::Oanda => (
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
            ),
            Mode::Binance => {
                let market_service = BinanceMarketDataService::builder()
                    .api_key(config.binance_api_key.clone())
                    .api_secret(config.binance_secret_key.clone())
                    .base_url(config.binance_base_url.clone())
                    .ws_url(config.binance_ws_url.clone())
                    .candle_repository(candle_repo)
                    .build();

                let spread_cache = market_service.get_spread_cache();

                let execution_service = BinanceExecutionService::new(
                    config.binance_api_key.clone(),
                    config.binance_secret_key.clone(),
                    config.binance_base_url.clone(),
                );

                (
                    Arc::new(market_service),
                    Arc::new(execution_service),
                    spread_cache,
                )
            }
        }
    }
}
