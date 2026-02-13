use crate::application::market_data::spread_cache::SpreadCache;
use crate::config::{Config, Mode};
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::portfolio::Portfolio;
use crate::infrastructure::alpaca::{AlpacaExecutionService, AlpacaMarketDataService};
use crate::infrastructure::binance::{BinanceExecutionService, BinanceMarketDataService};
use crate::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use crate::infrastructure::observability::Metrics;
use rust_decimal::prelude::ToPrimitive;
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

                let market_data: Arc<dyn MarketDataService> = if config.use_real_market_data {
                    match config.asset_class {
                        crate::config::AssetClass::Crypto => {
                            // Prefer Binance for crypto data quality
                            Arc::new(
                                BinanceMarketDataService::builder()
                                    .api_key(config.binance_api_key.clone())
                                    .base_url(config.binance_base_url.clone())
                                    .ws_url(config.binance_ws_url.clone())
                                    .build(),
                            )
                        }
                        crate::config::AssetClass::Stock => {
                            Arc::new(
                                AlpacaMarketDataService::builder()
                                    .api_key(config.alpaca_api_key.clone())
                                    .api_secret(config.alpaca_secret_key.clone())
                                    // Removed invalid .base_url call
                                    .data_base_url(config.alpaca_data_url.clone())
                                    .api_base_url(config.alpaca_base_url.clone())
                                    .min_volume_threshold(
                                        config.min_volume_threshold.to_f64().unwrap_or(10000.0),
                                    )
                                    .asset_class(config.asset_class)
                                    .build(),
                            )
                        }
                    }
                } else {
                    Arc::new(MockMarketDataService::new())
                };

                (
                    market_data,
                    Arc::new(execution_service),
                    Arc::new(SpreadCache::new()),
                )
            }
            Mode::Alpaca => {
                let market_service = AlpacaMarketDataService::builder()
                    .api_key(config.alpaca_api_key.clone())
                    .api_secret(config.alpaca_secret_key.clone())
                    .ws_url(config.alpaca_ws_url.clone())
                    .ws_url(config.alpaca_ws_url.clone())
                    .data_base_url(config.alpaca_data_url.clone())
                    .api_base_url(config.alpaca_base_url.clone())
                    .min_volume_threshold(config.min_volume_threshold.to_f64().unwrap_or(10000.0))
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
            Mode::Oanda => {
                // OANDA market data and execution not implemented; use Mock for now.
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
                        portfolio.clone(),
                        config.create_fee_model(),
                        latency_model,
                        slippage_model,
                    )
                } else {
                    MockExecutionService::with_costs(portfolio.clone(), config.create_fee_model())
                };
                (
                    Arc::new(MockMarketDataService::new()),
                    Arc::new(execution_service),
                    Arc::new(SpreadCache::new()),
                )
            }
            Mode::Binance => {
                let market_service = BinanceMarketDataService::builder()
                    .api_key(config.binance_api_key.clone())
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
