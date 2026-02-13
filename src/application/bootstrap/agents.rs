use anyhow::Result;
use chrono::Timelike;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{error, info, warn};

use crate::application::agents::{
    analyst::{Analyst, AnalystCommand, AnalystConfig, AnalystDependencies},
    executor::Executor,
    listener::ListenerAgent,
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
use crate::domain::sentiment::SentimentProvider;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Candle, TradeProposal};
use crate::infrastructure::alpaca::AlpacaSectorProvider;
use crate::infrastructure::binance::BinanceSectorProvider;
use crate::infrastructure::news::mock_news::MockNewsService;
use crate::infrastructure::news::rss::RssNewsService;
use crate::infrastructure::oanda::OandaSectorProvider;
use crate::infrastructure::observability::Metrics;
use crate::infrastructure::sentiment::alternative_me::AlternativeMeSentimentProvider;

// We need a struct to return all the control channels
pub struct AgentsHandle {
    pub sentinel_cmd_tx: mpsc::Sender<SentinelCommand>,
    pub risk_cmd_tx: mpsc::Sender<RiskCommand>,
    pub analyst_cmd_tx: mpsc::Sender<AnalystCommand>,
    pub proposal_tx: mpsc::Sender<TradeProposal>,
    pub candle_rx: broadcast::Receiver<Candle>,
    pub sentiment_rx: broadcast::Receiver<Sentiment>,
    pub news_rx: broadcast::Receiver<NewsEvent>,
}

pub struct AgentsBootstrap;

impl AgentsBootstrap {
    pub async fn init(
        config: &Config,
        services: &ServicesHandle,
        persistence: &PersistenceHandle,
        portfolio: Arc<RwLock<Portfolio>>,
        connection_health_service: Arc<ConnectionHealthService>,
        metrics: Metrics,
        agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
    ) -> Result<AgentsHandle> {
        info!("Initializing Agents...");

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
        let (sentiment_broadcast_tx, sentiment_broadcast_rx) = broadcast::channel(8);
        let (news_broadcast_tx, news_broadcast_rx) = broadcast::channel(20);

        // 1. Sentinel
        let mut sentinel = Sentinel::new(
            services.market_service.clone(),
            market_tx,
            config.symbols.clone(),
            Some(sentinel_cmd_rx),
            connection_health_service.clone(),
            agent_registry.clone(),
        );

        // 2. Market Scanner
        let scanner_interval =
            std::time::Duration::from_secs(config.dynamic_scan_interval_minutes * 60);
        let scanner = MarketScanner::new(
            services.market_service.clone(),
            services.execution_service.clone(),
            sentinel_cmd_tx.clone(),
            scanner_interval,
            config.dynamic_symbol_mode,
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
                ui_candle_tx: Some(candle_tx),
                spread_cache: services.spread_cache.clone(),
                connection_health_service: connection_health_service.clone(),
                agent_registry: agent_registry.clone(),
            },
        );

        // 4. Risk Manager
        let sector_provider: Option<Arc<dyn crate::domain::ports::SectorProvider>> =
            match config.mode {
                Mode::Alpaca => Some(Arc::new(AlpacaSectorProvider::new(
                    config.alpaca_api_key.clone(),
                    config.alpaca_secret_key.clone(),
                    config.alpaca_base_url.clone(),
                ))),
                Mode::Mock => None,
                Mode::Oanda => Some(Arc::new(OandaSectorProvider)),
                Mode::Binance => Some(Arc::new(BinanceSectorProvider)),
            };

        let base_risk = if config.asset_class == crate::config::AssetClass::Crypto {
            crate::domain::risk::risk_config::RiskConfig::crypto_default()
        } else {
            crate::domain::risk::risk_config::RiskConfig::default()
        };

        // When risk appetite is set, it drives all risk limits (prise de risque).
        let risk_config = if let Some(ref ra) = config.risk_appetite {
            crate::domain::risk::risk_config::RiskConfig {
                max_position_size_pct: ra.calculate_max_position_size_pct(),
                max_daily_loss_pct: ra.calculate_max_daily_loss_pct(),
                max_drawdown_pct: ra.calculate_max_drawdown_pct(),
                consecutive_loss_limit: ra.calculate_consecutive_loss_limit(),
                valuation_interval_seconds: base_risk.valuation_interval_seconds,
                max_sector_exposure_pct: config.max_sector_exposure_pct,
                sector_provider: sector_provider.clone(),
                pending_order_ttl_ms: config.pending_order_ttl_ms,
                allow_pdt_risk: base_risk.allow_pdt_risk,
                correlation_config: base_risk.correlation_config.clone(),
                volatility_config: base_risk.volatility_config.clone(),
            }
        } else {
            crate::domain::risk::risk_config::RiskConfig {
                max_position_size_pct: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.max_position_size_pct
                } else {
                    config.max_position_size_pct
                },
                max_daily_loss_pct: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.max_daily_loss_pct
                } else {
                    config.max_daily_loss_pct
                },
                max_drawdown_pct: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.max_drawdown_pct
                } else {
                    config.max_drawdown_pct
                },
                consecutive_loss_limit: if config.asset_class == crate::config::AssetClass::Crypto {
                    base_risk.consecutive_loss_limit
                } else {
                    config.consecutive_loss_limit
                },
                valuation_interval_seconds: base_risk.valuation_interval_seconds,
                max_sector_exposure_pct: config.max_sector_exposure_pct,
                sector_provider,
                pending_order_ttl_ms: config.pending_order_ttl_ms,
                allow_pdt_risk: base_risk.allow_pdt_risk,
                correlation_config: base_risk.correlation_config,
                volatility_config: base_risk.volatility_config,
            }
        };

        let correlation_svc = Arc::new(CorrelationService::new(
            persistence.candle_repository.clone(),
        ));

        // Start background refresh task
        correlation_svc
            .clone()
            .start_background_refresh(config.symbols.clone())
            .await;

        let correlation_service = Some(correlation_svc);

        let portfolio_state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                services.execution_service.clone(),
                config.portfolio_staleness_ms.try_into().unwrap_or(5000),
            ),
        );

        let mut risk_manager = RiskManager::new(
            proposal_rx,
            risk_cmd_rx,
            order_tx,
            services.execution_service.clone(),
            services.market_service.clone(),
            portfolio_state_manager,
            config.non_pdt_mode,
            config.asset_class,
            risk_config,
            services.performance_monitor.clone(),
            correlation_service,
            Some(persistence.risk_state_repository.clone()),
            Some(persistence.candle_repository.clone()),
            services.spread_cache.clone(),
            connection_health_service.clone(),
            metrics.clone(),
            agent_registry.clone(),
        )?;

        // 5. Order Throttler & Executor
        let mut order_throttler = OrderThrottler::new(
            order_rx,
            throttled_order_tx,
            config.max_orders_per_minute,
            agent_registry.clone(),
        );

        let retry_config = crate::application::risk_management::order_retry_strategy::RetryConfig {
            limit_timeout_ms: config.pending_order_ttl_ms.unwrap_or(5000) as u64,
            enable_retry: true,
        };

        let mut executor = Executor::new(
            services.execution_service.clone(),
            throttled_order_rx,
            portfolio.clone(),
            Some(persistence.order_repository.clone()),
            retry_config,
            connection_health_service.clone(),
            config.create_fee_model(),
            agent_registry.clone(),
        );

        // SPAWN TASKS
        tokio::spawn(async move { sentinel.run().await });
        tokio::spawn(async move { scanner.run().await });
        tokio::spawn(async move { analyst.run().await });
        tokio::spawn(async move { risk_manager.run().await });
        tokio::spawn(async move { order_throttler.run().await });
        tokio::spawn(async move { executor.run().await });

        // Listener Agent
        spawn_listener(
            analyst_cmd_tx.clone(),
            news_broadcast_tx.clone(),
            agent_registry.clone(),
        );

        // Sentiment Polling
        spawn_sentiment_poller(
            config,
            risk_cmd_tx.clone(),
            sentiment_broadcast_tx,
            metrics.clone(),
        );

        // Adaptive Optimization
        spawn_adaptive_optimization(config, services.adaptive_optimization_service.clone());

        Ok(AgentsHandle {
            sentinel_cmd_tx,
            risk_cmd_tx,
            analyst_cmd_tx,
            proposal_tx,
            candle_rx,
            sentiment_rx: sentiment_broadcast_rx,
            news_rx: news_broadcast_rx,
        })
    }
}

// Helper functions to keep init clean

fn create_analyst_config(config: &Config) -> AnalystConfig {
    use rust_decimal_macros::dec;

    AnalystConfig {
        fast_sma_period: config.fast_sma_period,
        slow_sma_period: config.slow_sma_period,
        max_positions: config.max_positions,
        trade_quantity: config.trade_quantity,
        sma_threshold: config.sma_threshold,
        order_cooldown_seconds: config.order_cooldown_seconds,
        risk_per_trade_percent: config.risk_per_trade_percent,
        strategy_mode: config.strategy_mode,
        trend_sma_period: config.trend_sma_period,
        rsi_period: config.rsi_period,
        macd_fast_period: config.macd_fast_period,
        macd_slow_period: config.macd_slow_period,
        macd_signal_period: config.macd_signal_period,
        trend_divergence_threshold: config.trend_divergence_threshold,
        trailing_stop_atr_multiplier: config.trailing_stop_atr_multiplier,
        atr_period: config.atr_period,
        rsi_threshold: config.rsi_threshold,
        trend_riding_exit_buffer_pct: dec!(0.03),
        mean_reversion_rsi_exit: config.mean_reversion_rsi_exit,
        mean_reversion_bb_period: config.mean_reversion_bb_period,
        fee_model: config.create_fee_model(),
        max_position_size_pct: config.max_position_size_pct,
        bb_std_dev: dec!(2.0),
        ema_fast_period: config.ema_fast_period,
        ema_slow_period: config.ema_slow_period,
        take_profit_pct: config.take_profit_pct,
        min_hold_time_minutes: config.min_hold_time_minutes,
        signal_confirmation_bars: config.signal_confirmation_bars,
        spread_bps: config.spread_bps,
        min_profit_ratio: config.min_profit_ratio,
        profit_target_multiplier: config.profit_target_multiplier,
        macd_requires_rising: config.macd_requires_rising,
        trend_tolerance_pct: config.trend_tolerance_pct,
        macd_min_threshold: config.macd_min_threshold,
        adx_period: config.adx_period,
        adx_threshold: config.adx_threshold,
        smc_ob_lookback: config.smc_ob_lookback,
        smc_min_fvg_size_pct: config.smc_min_fvg_size_pct,
        smc_volume_multiplier: dec!(1.5),
        risk_appetite_score: config.risk_appetite.map(|r| r.score()),
        breakout_lookback: 10,
        breakout_threshold_pct: dec!(0.002),
        breakout_volume_mult: dec!(1.1),
        max_loss_per_trade_pct: dec!(-0.05),
        enable_ml_data_collection: false,
        stat_momentum_lookback: 10,
        stat_momentum_threshold: dec!(1.5),
        stat_momentum_trend_confirmation: true,
        zscore_lookback: 20,
        zscore_entry_threshold: dec!(-2.0),
        zscore_exit_threshold: dec!(0.0),
        orderflow_ofi_threshold: dec!(0.3),
        orderflow_stacked_count: 3,
        orderflow_volume_profile_lookback: 100,
        ensemble_weights: None,
        ensemble_voting_threshold: config.ensemble_voting_threshold,
    }
}

fn create_strategy(config: &Config, analyst_config: &AnalystConfig) -> Arc<dyn TradingStrategy> {
    match config.strategy_mode {
        crate::domain::market::strategy_config::StrategyMode::Standard => {
            Arc::new(DualSMAStrategy::new(
                config.fast_sma_period,
                config.slow_sma_period,
                config.sma_threshold,
            ))
        }
        crate::domain::market::strategy_config::StrategyMode::Advanced => Arc::new(
            AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
                fast_period: analyst_config.fast_sma_period,
                slow_period: analyst_config.slow_sma_period,
                sma_threshold: analyst_config.sma_threshold,
                trend_sma_period: analyst_config.trend_sma_period,
                rsi_threshold: analyst_config.rsi_threshold,
                signal_confirmation_bars: analyst_config.signal_confirmation_bars,
                macd_requires_rising: analyst_config.macd_requires_rising,
                trend_tolerance_pct: analyst_config.trend_tolerance_pct,
                macd_min_threshold: analyst_config.macd_min_threshold,
                adx_threshold: analyst_config.adx_threshold,
            }),
        ),
        crate::domain::market::strategy_config::StrategyMode::Dynamic => {
            Arc::new(DynamicRegimeStrategy::with_config(DynamicRegimeConfig {
                fast_period: analyst_config.fast_sma_period,
                slow_period: analyst_config.slow_sma_period,
                sma_threshold: analyst_config.sma_threshold,
                trend_sma_period: analyst_config.trend_sma_period,
                rsi_threshold: analyst_config.rsi_threshold,
                trend_divergence_threshold: analyst_config.trend_divergence_threshold,
                signal_confirmation_bars: analyst_config.signal_confirmation_bars,
                macd_requires_rising: analyst_config.macd_requires_rising,
                trend_tolerance_pct: analyst_config.trend_tolerance_pct,
                macd_min_threshold: analyst_config.macd_min_threshold,
                adx_threshold: analyst_config.adx_threshold,
            }))
        }
        crate::domain::market::strategy_config::StrategyMode::TrendRiding => {
            Arc::new(TrendRidingStrategy::new(
                config.fast_sma_period,
                config.slow_sma_period,
                config.sma_threshold,
                config.trend_riding_exit_buffer_pct,
            ))
        }
        crate::domain::market::strategy_config::StrategyMode::MeanReversion => {
            Arc::new(MeanReversionStrategy::new(
                analyst_config.mean_reversion_bb_period,
                analyst_config.mean_reversion_rsi_exit,
            ))
        }
        crate::domain::market::strategy_config::StrategyMode::RegimeAdaptive => {
            Arc::new(crate::application::strategies::TrendRidingStrategy::new(
                analyst_config.fast_sma_period,
                analyst_config.slow_sma_period,
                analyst_config.sma_threshold,
                analyst_config.trend_riding_exit_buffer_pct,
            ))
        }
        crate::domain::market::strategy_config::StrategyMode::SMC => Arc::new(SMCStrategy::new(
            analyst_config.smc_ob_lookback,
            analyst_config.smc_min_fvg_size_pct,
            analyst_config.smc_volume_multiplier,
        )),
        crate::domain::market::strategy_config::StrategyMode::VWAP => {
            Arc::new(VWAPStrategy::default())
        }
        crate::domain::market::strategy_config::StrategyMode::Breakout => {
            Arc::new(BreakoutStrategy::default())
        }
        crate::domain::market::strategy_config::StrategyMode::Momentum => {
            Arc::new(MomentumDivergenceStrategy::default())
        }
        crate::domain::market::strategy_config::StrategyMode::Ensemble => {
            Arc::new(EnsembleStrategy::modern_ensemble(analyst_config))
        }
        crate::domain::market::strategy_config::StrategyMode::ZScoreMR => {
            Arc::new(ZScoreMeanReversionStrategy::new(
                analyst_config.zscore_lookback,
                analyst_config.zscore_entry_threshold,
                analyst_config.zscore_exit_threshold,
            ))
        }
        crate::domain::market::strategy_config::StrategyMode::StatMomentum => {
            Arc::new(StatisticalMomentumStrategy::new(
                analyst_config.stat_momentum_lookback,
                analyst_config.stat_momentum_threshold,
                analyst_config.stat_momentum_trend_confirmation,
            ))
        }
        crate::domain::market::strategy_config::StrategyMode::OrderFlow => {
            Arc::new(OrderFlowStrategy::new(
                analyst_config.orderflow_ofi_threshold,
                analyst_config.orderflow_stacked_count,
                analyst_config.orderflow_volume_profile_lookback,
            ))
        }
        crate::domain::market::strategy_config::StrategyMode::ML => {
            let path = std::path::PathBuf::from("data/ml/model.bin");
            let predictor =
                crate::application::ml::smartcore_predictor::SmartCorePredictor::new(path);
            Arc::new(MLStrategy::new(Arc::new(Box::new(predictor)), 0.0005))
        }
    }
}

fn spawn_listener(
    logger_analyst_tx: mpsc::Sender<AnalystCommand>,
    news_tx_for_listener: broadcast::Sender<NewsEvent>,
    agent_registry: Arc<crate::application::monitoring::agent_status::AgentStatusRegistry>,
) {
    tokio::spawn(async move {
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

        let news_rss_url = std::env::var("NEWS_RSS_URL").ok();

        let news_service: Arc<dyn crate::domain::ports::NewsDataService> =
            if let Some(url) = news_rss_url {
                info!("Using RSS News Service with URL: {}", url);
                Arc::new(RssNewsService::new(&url, 60))
            } else {
                info!("Using Mock News Service (NEWS_RSS_URL not set)");
                Arc::new(MockNewsService::new())
            };

        let listener = ListenerAgent::with_news_broadcast(
            news_service,
            config,
            logger_analyst_tx, // Fixed variable name matching
            news_tx_for_listener,
            agent_registry,
        );
        listener.run().await;
    });
}

fn spawn_sentiment_poller(
    config: &Config,
    sentiment_tx: mpsc::Sender<RiskCommand>,
    sentiment_broadcast_tx: broadcast::Sender<Sentiment>,
    metrics: Metrics,
) {
    let asset_class = config.asset_class;
    tokio::spawn(async move {
        // Only poll for Crypto for now as we use Alternative.me
        // In future we can add VIX for stocks
        if asset_class == crate::config::AssetClass::Crypto {
            info!("Starting Sentiment Polling Task (Alternative.me)...");
            let provider = AlternativeMeSentimentProvider::new();

            // Initial fetch
            if let Ok(sentiment) = provider.fetch_sentiment().await {
                let _ = sentiment_tx
                    .send(RiskCommand::UpdateSentiment(sentiment.clone()))
                    .await;
                metrics.sentiment_score.set(sentiment.value as f64);
                let _ = sentiment_broadcast_tx.send(sentiment);
            }

            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(4 * 3600)).await; // Every 4 hours
                match provider.fetch_sentiment().await {
                    Ok(sentiment) => {
                        if let Err(e) = sentiment_tx
                            .send(RiskCommand::UpdateSentiment(sentiment.clone()))
                            .await
                        {
                            error!("Failed to send sentiment update: {}", e);
                        }
                        metrics.sentiment_score.set(sentiment.value as f64);
                        let _ = sentiment_broadcast_tx.send(sentiment);
                    }
                    Err(e) => {
                        warn!("Failed to fetch sentiment: {}", e);
                    }
                }
            }
        } else {
            // For Stock mode, send a mock neutral sentiment for UI display
            info!("Asset class is Stock - using mock neutral sentiment for UI");
            let mock_sentiment = crate::domain::sentiment::Sentiment {
                value: 50,
                classification: crate::domain::sentiment::SentimentClassification::Neutral,
                timestamp: chrono::Utc::now(),
                source: "Mock (Stock Mode)".to_string(),
            };
            let _ = sentiment_broadcast_tx.send(mock_sentiment);
        }
    });
}

fn spawn_adaptive_optimization(
    config: &Config,
    adaptive_service: Option<Arc<crate::application::optimization::adaptive_optimization_service::AdaptiveOptimizationService>>,
) {
    let symbols = config.symbols.clone();
    let eval_hour = config.adaptive_evaluation_hour;

    tokio::spawn(async move {
        if let Some(service) = adaptive_service {
            info!(
                "Starting Adaptive Optimization Service task (Evaluation hour: {:02}:00 UTC)",
                eval_hour
            );
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
        }
    });
}
