use anyhow::Result;
use chrono::Timelike;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::application::agents::{
    analyst::{Analyst, AnalystCommand, AnalystConfig, AnalystDependencies},
    executor::Executor,
    listener::{ListenerAgent, ListenerCommand},
    scanner::MarketScanner,
    sentinel::{Sentinel, SentinelCommand},
};
use crate::application::bootstrap::persistence::PersistenceHandle;
use crate::application::bootstrap::services::ServicesHandle;
use crate::application::monitoring::connection_health_service::ConnectionHealthService;
use crate::application::monitoring::correlation_service::CorrelationService;
use crate::application::optimization::win_rate_provider::HistoricalWinRateProvider;
use crate::application::risk_management::{
    commands::RiskCommand, order_throttler::OrderThrottler, risk_manager::RiskManager,
};
use crate::application::strategies::*;
use crate::config::{Config, Mode};
use crate::domain::listener::NewsEvent;
use crate::domain::listener::{ListenerAction, ListenerConfig};
use crate::domain::sentiment::Sentiment;

use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Candle, TradeProposal};
use crate::infrastructure::alpaca::AlpacaSectorProvider;
use crate::infrastructure::binance::BinanceSectorProvider;
use crate::infrastructure::news::rss::RssNewsService;
#[cfg(feature = "oanda")]
use crate::infrastructure::oanda::OandaSectorProvider;
use crate::infrastructure::observability::Metrics;

// We need a struct to return all the control channels
pub struct AgentsHandle {
    pub sentinel_cmd_tx: mpsc::Sender<SentinelCommand>,
    pub risk_cmd_tx: mpsc::Sender<RiskCommand>,
    pub analyst_cmd_tx: mpsc::Sender<AnalystCommand>,
    pub listener_cmd_tx: Option<mpsc::Sender<ListenerCommand>>,
    pub proposal_tx: mpsc::Sender<TradeProposal>,
    pub candle_rx: broadcast::Receiver<Candle>,
    pub sentiment_rx: broadcast::Receiver<Sentiment>,
    pub news_rx: Option<broadcast::Receiver<NewsEvent>>,
}

pub struct AgentsBootstrap;

impl AgentsBootstrap {
    #[allow(clippy::too_many_arguments)]
    pub async fn init(
        config: &Config,
        services: &ServicesHandle,
        persistence: &PersistenceHandle,
        portfolio: Arc<RwLock<Portfolio>>,
        connection_health_service: Arc<ConnectionHealthService>,
        metrics: Metrics,
        agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
        cancel_token: CancellationToken,
    ) -> Result<(AgentsHandle, tokio::task::JoinSet<()>)> {
        info!("Initializing Agents...");
        let mut join_set = tokio::task::JoinSet::new();

        // Channel creation
        let (market_tx, market_rx) = mpsc::channel(500);
        let (proposal_tx, proposal_rx) = mpsc::channel(100);
        let (order_tx, order_rx) = mpsc::channel(50);
        let (throttled_order_tx, throttled_order_rx) = mpsc::channel(50);
        let (sentinel_cmd_tx, sentinel_cmd_rx) = mpsc::channel(10);
        let (risk_cmd_tx, risk_cmd_rx) = mpsc::channel(10);
        let (analyst_cmd_tx, analyst_cmd_rx) = mpsc::channel(10);

        // Broadcast channels
        let (candle_tx, candle_rx) = broadcast::channel(100);
        let (sentiment_broadcast_tx, _sentiment_broadcast_rx) = broadcast::channel(8);

        // 1. Sentinel
        let mut sentinel = Sentinel::new(
            services.market_service.clone(),
            market_tx,
            config.platform.symbols.clone(),
            Some(sentinel_cmd_rx),
            connection_health_service.clone(),
            agent_registry.clone(),
        );

        // 2. Market Scanner
        let scanner_interval =
            std::time::Duration::from_secs(config.platform.dynamic_scan_interval_minutes * 60);
        let scanner = MarketScanner::new(
            services.market_service.clone(),
            services.execution_service.clone(),
            sentinel_cmd_tx.clone(),
            scanner_interval,
            config.platform.dynamic_symbol_mode,
            agent_registry.clone(),
        );

        // 3. Analyst
        let analyst_config = create_analyst_config(config);
        let strategy = create_strategy(config, &analyst_config);

        let win_rate_provider = Arc::new(HistoricalWinRateProvider::new(
            persistence.order_repository.clone(),
            0.50,
            10,
        ));

        let mut analyst = Analyst::new(
            market_rx,
            analyst_cmd_rx,
            proposal_tx.clone(),
            analyst_config.clone(), // Clone needed for logging/debug if used later, or just use config
            strategy,
            AnalystDependencies {
                execution_service: services.execution_service.clone(),
                market_service: services.market_service.clone(),
                candle_repository: Some(persistence.candle_repository.clone()),
                strategy_repository: Some(persistence.strategy_repository.clone()),
                win_rate_provider: Some(win_rate_provider),
                ui_candle_tx: Some(candle_tx.clone()),
                spread_cache: services.spread_cache.clone(),
                connection_health_service: connection_health_service.clone(),
                agent_registry: agent_registry.clone(),
            },
        );

        // 4. Risk Manager
        let sector_provider: Option<Arc<dyn crate::domain::ports::SectorProvider>> = match config
            .mode
        {
            Mode::Alpaca => Some(Arc::new(AlpacaSectorProvider::new(
                config.broker.alpaca.api_key.clone(),
                config.broker.alpaca.secret_key.clone(),
                config.broker.alpaca.base_url.clone(),
            ))),
            Mode::Mock => None,
            Mode::Oanda => {
                #[cfg(feature = "oanda")]
                {
                    Some(Arc::new(OandaSectorProvider))
                }
                #[cfg(not(feature = "oanda"))]
                {
                    panic!("Oanda support is disabled. Compile with --features oanda to enable.");
                }
            }
            Mode::Binance => Some(Arc::new(BinanceSectorProvider)),
        };

        let base_risk = if config.asset_class == crate::config::AssetClass::Crypto {
            crate::domain::risk::risk_config::RiskConfig::crypto_default()
        } else {
            crate::domain::risk::risk_config::RiskConfig::default()
        };

        use crate::domain::risk::risk_appetite::RiskAppetite;
        // When risk appetite is set, it drives all risk limits (prise de risque).
        let risk_config = if let Some(score) = config.strategy.risk_appetite_score {
            if let Ok(ra) = RiskAppetite::new(score) {
                crate::domain::risk::risk_config::RiskConfig {
                    max_position_size_pct: ra.calculate_max_position_size_pct(),
                    max_daily_loss_pct: ra.calculate_max_daily_loss_pct(),
                    max_drawdown_pct: ra.calculate_max_drawdown_pct(),
                    consecutive_loss_limit: ra.calculate_consecutive_loss_limit(),
                    valuation_interval_seconds: base_risk.valuation_interval_seconds,
                    max_sector_exposure_pct: config.risk.max_sector_exposure_pct,
                    sector_provider: sector_provider.clone(),
                    pending_order_ttl_ms: config.risk.pending_order_ttl_ms,
                    allow_pdt_risk: base_risk.allow_pdt_risk,
                    correlation_config: base_risk.correlation_config.clone(),
                    volatility_config: base_risk.volatility_config.clone(),

                    max_positions: config.risk.max_positions,
                    risk_per_trade_percent: config.risk.risk_per_trade_percent,
                    trade_quantity: config.risk.trade_quantity,
                    order_cooldown_seconds: config.risk.order_cooldown_seconds,
                    max_orders_per_minute: config.risk.max_orders_per_minute,
                    min_hold_time_minutes: config.risk.min_hold_time_minutes,
                    max_loss_per_trade_pct: config.risk.max_loss_per_trade_pct,
                }
            } else {
                base_risk.clone()
            }
        } else {
            crate::domain::risk::risk_config::RiskConfig {
                max_position_size_pct: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.max_position_size_pct
                } else {
                    config.risk.max_position_size_pct
                },
                max_daily_loss_pct: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.max_daily_loss_pct
                } else {
                    config.risk.max_daily_loss_pct
                },
                max_drawdown_pct: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.max_drawdown_pct
                } else {
                    config.risk.max_drawdown_pct
                },
                consecutive_loss_limit: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.consecutive_loss_limit
                } else {
                    config.risk.consecutive_loss_limit
                },
                valuation_interval_seconds: base_risk.valuation_interval_seconds,
                max_sector_exposure_pct: config.risk.max_sector_exposure_pct,
                sector_provider,
                pending_order_ttl_ms: config.risk.pending_order_ttl_ms,
                allow_pdt_risk: base_risk.allow_pdt_risk,
                correlation_config: base_risk.correlation_config,
                volatility_config: base_risk.volatility_config,

                max_positions: config.risk.max_positions,
                risk_per_trade_percent: config.risk.risk_per_trade_percent,
                trade_quantity: config.risk.trade_quantity,
                order_cooldown_seconds: config.risk.order_cooldown_seconds,
                max_orders_per_minute: config.risk.max_orders_per_minute,
                min_hold_time_minutes: config.risk.min_hold_time_minutes,
                max_loss_per_trade_pct: config.risk.max_loss_per_trade_pct,
            }
        };

        let correlation_svc = Arc::new(CorrelationService::new(
            persistence.candle_repository.clone(),
        ));

        // Start background refresh task
        correlation_svc
            .clone()
            .start_background_refresh(config.platform.symbols.clone())
            .await;

        let correlation_service = Some(correlation_svc);

        let portfolio_state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                services.execution_service.clone(),
                config
                    .platform
                    .portfolio_staleness_ms
                    .try_into()
                    .unwrap_or(5000),
            ),
        );

        use crate::application::risk_management::risk_manager::RiskManagerDependencies;
        let mut risk_manager = RiskManager::new(
            proposal_rx,
            risk_cmd_rx,
            order_tx,
            config.platform.non_pdt_mode,
            config.asset_class,
            risk_config,
            RiskManagerDependencies {
                execution_service: services.execution_service.clone(),
                market_service: services.market_service.clone(),
                portfolio_state_manager,
                performance_monitor: services.performance_monitor.clone(),
                correlation_service,
                risk_state_repository: Some(persistence.risk_state_repository.clone()),
                candle_repository: Some(persistence.candle_repository.clone()),
                spread_cache: services.spread_cache.clone(),
                connection_health_service: connection_health_service.clone(),
                metrics: metrics.clone(),
                agent_registry: agent_registry.clone(),
            },
        )?;
        risk_manager.set_notification_service(Arc::new(
            crate::infrastructure::notifications::MultiChannelNotificationService::from_env(),
        ));
        risk_manager.set_alert_webhook_url(config.observability.alert_webhook_url.clone());

        // 5. Order Throttler & Executor
        let mut order_throttler = OrderThrottler::new(
            order_rx,
            throttled_order_tx,
            config.risk.max_orders_per_minute,
            agent_registry.clone(),
        );

        let retry_config = crate::application::risk_management::order_retry_strategy::RetryConfig {
            limit_timeout_ms: config.risk.pending_order_ttl_ms.unwrap_or(5000) as u64,
            enable_retry: true,
        };

        use crate::application::agents::executor::ExecutorDependencies;
        let mut executor = Executor::new(
            throttled_order_rx,
            portfolio.clone(),
            ExecutorDependencies {
                execution_service: services.execution_service.clone(),
                repository: Some(persistence.order_repository.clone()),
                retry_config,
                health_service: connection_health_service.clone(),
                fee_model: config.create_fee_model(),
                agent_registry: agent_registry.clone(),
                candle_rx: Some(candle_tx.subscribe()),
            },
        );

        // SPAWN TASKS
        let ct1 = cancel_token.clone();
        join_set.spawn(async move {
            tokio::select! { _ = sentinel.run() => {}, _ = ct1.cancelled() => {} }
        });
        let ct2 = cancel_token.clone();
        join_set.spawn(async move {
            tokio::select! { _ = scanner.run() => {}, _ = ct2.cancelled() => {} }
        });
        let ct3 = cancel_token.clone();
        join_set.spawn(async move {
            tokio::select! { _ = analyst.run() => {}, _ = ct3.cancelled() => {} }
        });
        let ct4 = cancel_token.clone();
        join_set.spawn(async move {
            tokio::select! { _ = risk_manager.run() => {}, _ = ct4.cancelled() => {} }
        });
        let ct5 = cancel_token.clone();
        join_set.spawn(async move {
            tokio::select! { _ = order_throttler.run() => {}, _ = ct5.cancelled() => {} }
        });
        let ct6 = cancel_token.clone();
        join_set.spawn(async move {
            tokio::select! { _ = executor.run() => {}, _ = ct6.cancelled() => {} }
        });

        // Spawn periodic heartbeat update for ConnectionHealthService
        let health_service_for_heartbeat = connection_health_service.clone();
        let registry_for_heartbeat = agent_registry.clone();
        let ct_health = cancel_token.clone();
        join_set.spawn(async move {
            loop {
                let m_status = health_service_for_heartbeat.get_market_data_status().await;
                let e_status = health_service_for_heartbeat.get_execution_status().await;

                use crate::application::monitoring::connection_health_service::ConnectionStatus;
                let health = if m_status == ConnectionStatus::Offline
                    && e_status == ConnectionStatus::Offline
                {
                    crate::application::monitoring::agent_status::HealthStatus::Dead
                } else if m_status == ConnectionStatus::Degraded
                    || e_status == ConnectionStatus::Degraded
                {
                    crate::application::monitoring::agent_status::HealthStatus::Degraded
                } else {
                    crate::application::monitoring::agent_status::HealthStatus::Healthy
                };

                registry_for_heartbeat
                    .update_heartbeat("ConnectionHealthService", health)
                    .await;

                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    _ = ct_health.cancelled() => break,
                }
            }
        });

        // Listener Agent (Optional)
        // Check if settings have RSS URLs, fallback to .env for backward compatibility
        let persisted_settings =
            crate::infrastructure::settings_persistence::SettingsPersistence::new()
                .ok()
                .and_then(|p| p.load().unwrap_or(None));
        let mut rss_urls = persisted_settings
            .as_ref()
            .map(|s| s.news.rss_urls.clone())
            .unwrap_or_default();

        if rss_urls.is_empty()
            && let Ok(url_str) = std::env::var("NEWS_RSS_URL")
        {
            for url in url_str.split(',') {
                let trim = url.trim();
                if !trim.is_empty() {
                    rss_urls.push(trim.to_string());
                }
            }
        }

        // Always spawn listener so it can receive dynamic URL updates later
        let (news_broadcast_tx, news_broadcast_rx) = broadcast::channel(20);
        let (listener_cmd_tx, listener_cmd_rx) = mpsc::channel(10);
        spawn_listener(
            &mut join_set,
            rss_urls,
            listener_cmd_rx,
            analyst_cmd_tx.clone(),
            news_broadcast_tx.clone(),
            sentiment_broadcast_tx.clone(),
            agent_registry.clone(),
            cancel_token.clone(),
        );
        let news_rx = Some(news_broadcast_rx);
        let listener_cmd_tx = Some(listener_cmd_tx);

        // Forward Sentiment Broadcast to RiskManager
        let mut sentiment_rx_for_risk = sentiment_broadcast_tx.subscribe();
        let risk_tx_for_sentiment = risk_cmd_tx.clone();
        let ct7 = cancel_token.clone();
        join_set.spawn(async move {
            tokio::select! {
                _ = async {
                    while let Ok(sentiment) = sentiment_rx_for_risk.recv().await {
                        let _ = risk_tx_for_sentiment
                            .send(RiskCommand::UpdateSentiment(sentiment))
                            .await;
                    }
                    std::future::pending::<()>().await;
                } => {}
                _ = ct7.cancelled() => {}
            }
        });

        // Adaptive Optimization
        spawn_adaptive_optimization(
            &mut join_set,
            config,
            services.adaptive_optimization_service.clone(),
            cancel_token.clone(),
        );

        Ok((
            AgentsHandle {
                sentinel_cmd_tx,
                risk_cmd_tx,
                analyst_cmd_tx,
                listener_cmd_tx,
                proposal_tx,
                candle_rx,
                sentiment_rx: sentiment_broadcast_tx.subscribe(),
                news_rx,
            },
            join_set,
        ))
    }
}

// Helper functions to keep init clean

fn create_analyst_config(config: &Config) -> AnalystConfig {
    use crate::domain::risk::risk_appetite::RiskAppetite;

    let mut analyst_config = AnalystConfig::from(config.clone());

    // Apply risk appetite settings if present to override base values
    if let Some(score) = config.strategy.risk_appetite_score
        && let Ok(appetite) = RiskAppetite::new(score)
    {
        analyst_config.apply_risk_appetite(&appetite);
    }

    analyst_config
}

fn create_strategy(config: &Config, analyst_config: &AnalystConfig) -> Arc<dyn TradingStrategy> {
    // Delegate to the single source of truth for strategy creation.
    // This avoids duplicating match arms that diverge silently over time.
    crate::application::strategies::strategy_factory::StrategyFactory::create(
        config.strategy.strategy_mode,
        analyst_config,
    )
}

#[allow(clippy::too_many_arguments)]
fn spawn_listener(
    join_set: &mut tokio::task::JoinSet<()>,
    rss_urls: Vec<String>,
    listener_cmd_rx: mpsc::Receiver<ListenerCommand>,
    logger_analyst_tx: mpsc::Sender<AnalystCommand>,
    news_tx_for_listener: broadcast::Sender<NewsEvent>,
    sentiment_broadcast_tx: broadcast::Sender<Sentiment>,
    agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    cancel_token: CancellationToken,
) {
    join_set.spawn(async move {
        info!("Starting Listener Agent...");
        // Hardcoded configuration for now as per plan
        let config = ListenerConfig {
            poll_interval_seconds: 30, // Mock news service has its own internal delays
            rules: vec![
                crate::domain::listener::ListenerRule {
                    id: "elon-doge".to_string(),
                    keywords: vec!["Elon Musk".to_string(), "Dogecoin".to_string()],
                    target_symbol: "DOGE/USD".to_string(),
                    action: ListenerAction::NotifyAnalyst(
                        crate::domain::listener::NewsSentiment::Bullish,
                    ),
                    active: true,
                },
                crate::domain::listener::ListenerRule {
                    id: "sec-lawsuit".to_string(),
                    keywords: vec![
                        "SEC".to_string(),
                        "Lawsuit".to_string(),
                        "Binance".to_string(),
                    ],
                    target_symbol: "BNB/USD".to_string(), // Assuming Binance Coin or broad market selloff
                    action: ListenerAction::NotifyAnalyst(
                        crate::domain::listener::NewsSentiment::Bearish,
                    ),
                    active: true,
                },
            ],
        };

        let urls_lock = Arc::new(RwLock::new(rss_urls));
        let news_service: Arc<dyn crate::domain::ports::NewsDataService> =
            Arc::new(RssNewsService::new(urls_lock, 60));

        let listener = ListenerAgent::with_news_broadcast(
            news_service,
            config,
            listener_cmd_rx,
            logger_analyst_tx, // Fixed variable name matching
            news_tx_for_listener,
            sentiment_broadcast_tx,
            agent_registry,
        );
        tokio::select! {
            _ = listener.run() => {}
            _ = cancel_token.cancelled() => {}
        }
    });
}

fn spawn_adaptive_optimization(
    join_set: &mut tokio::task::JoinSet<()>,
    config: &Config,
    adaptive_service: Option<Arc<crate::application::optimization::adaptive_optimization_service::AdaptiveOptimizationService>>,
    cancel_token: CancellationToken,
) {
    let symbols = config.platform.symbols.clone();
    let eval_hour = config.platform.adaptive_evaluation_hour;

    if let Some(service) = adaptive_service {
        join_set.spawn(async move {
            info!(
                "Starting Adaptive Optimization Service task (Evaluation hour: {:02}:00 UTC)",
                eval_hour
            );
            tokio::select! {
                _ = async {
                    loop {
                        let now = chrono::Utc::now();
                        if now.hour() == eval_hour {
                            info!(
                                "Triggering daily adaptive evaluation for symbols: {:?}",
                                symbols
                            );
                            for symbol in &symbols {
                                if let Err(e) = service.run_daily_evaluation(symbol).await {
                                    error!("Adaptive Optimization failed for {}: {}", symbol, e);
                                }
                            }
                            // Sleep for an hour and a bit to avoid re-triggering immediately
                            tokio::time::sleep(tokio::time::Duration::from_secs(3660)).await;
                        } else {
                            // Check every 15 minutes
                            tokio::time::sleep(tokio::time::Duration::from_secs(900)).await;
                        }
                    }
                } => {}
                _ = cancel_token.cancelled() => {}
            }
        });
    }
}
