use anyhow::Result;
use chrono::Timelike;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{error, info, warn};

use crate::application::optimization::win_rate_provider::HistoricalWinRateProvider;
use crate::application::strategies::TradingStrategy;
use crate::application::{
    agents::{
        analyst::{Analyst, AnalystCommand, AnalystConfig, AnalystDependencies},
        executor::Executor,
        scanner::MarketScanner,
        sentinel::{Sentinel, SentinelCommand}, // Added SentinelCommand
    },
    market_data::spread_cache::SpreadCache,
    monitoring::{
        correlation_service::CorrelationService,
        performance_monitoring_service::PerformanceMonitoringService,
    },
    optimization::{
        adaptive_optimization_service::AdaptiveOptimizationService,
        optimizer::{GridSearchOptimizer, ParameterGrid},
    },
    risk_management::{
        order_throttler::OrderThrottler,
        risk_manager::RiskManager,
        commands::RiskCommand,
    },
    strategies::*,
};
use crate::config::{Config, Mode};

use crate::domain::performance::performance_evaluator::{
    EvaluationThresholds, PerformanceEvaluator,
};
use crate::domain::ports::SectorProvider;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::{CandleRepository, StrategyRepository, TradeRepository};
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::Candle;
use crate::domain::trading::types::TradeProposal; // Added TradeProposal import
use crate::domain::sentiment::Sentiment; // Added Sentiment import
use crate::infrastructure::alpaca::AlpacaSectorProvider;
use crate::infrastructure::binance::BinanceSectorProvider;
use crate::infrastructure::factory::ServiceFactory;
use crate::infrastructure::oanda::OandaSectorProvider;
use crate::infrastructure::persistence::database::Database;
use crate::infrastructure::persistence::repositories::{
    SqliteCandleRepository, SqliteOptimizationHistoryRepository, SqliteOrderRepository,
    SqlitePerformanceSnapshotRepository, SqliteReoptimizationTriggerRepository,
    SqliteStrategyRepository,
};
use crate::infrastructure::sentiment::alternative_me::AlternativeMeSentimentProvider;
use crate::domain::sentiment::SentimentProvider;

pub struct SystemHandle {
    pub sentinel_cmd_tx: mpsc::Sender<SentinelCommand>,
    pub risk_cmd_tx: mpsc::Sender<RiskCommand>,
    pub analyst_cmd_tx: mpsc::Sender<AnalystCommand>,
    pub proposal_tx: mpsc::Sender<TradeProposal>,
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub candle_rx: broadcast::Receiver<Candle>,
    pub sentiment_rx: broadcast::Receiver<Sentiment>, // Added sentiment rx
    pub strategy_mode: crate::domain::market::strategy_config::StrategyMode,
    pub risk_appetite: Option<crate::domain::risk::risk_appetite::RiskAppetite>,
}

pub struct Application {
    pub config: Config,
    pub market_service: Arc<dyn MarketDataService>,
    pub execution_service: Arc<dyn ExecutionService>,
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub order_repository: Arc<dyn TradeRepository>,
    pub candle_repository: Option<Arc<dyn CandleRepository>>,
    pub strategy_repository: Arc<dyn StrategyRepository>,
    pub adaptive_optimization_service: Option<Arc<AdaptiveOptimizationService>>,
    pub performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    pub spread_cache: Arc<SpreadCache>, // Shared spread cache from market data service
}

impl Application {
    pub async fn build(config: Config) -> Result<Self> {
        info!("Building Rustrade Application (Mode: {:?})...", config.mode);

        // 1. Initialize Shared State
        let mut initial_portfolio = Portfolio::new();
        initial_portfolio.cash = config.initial_cash;
        let portfolio = Arc::new(RwLock::new(initial_portfolio));

        // 2. Initialize Persistence FIRST (needed by AlpacaMarketDataService for caching)
        let db_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://rustrade.db".to_string());
        info!("Initializing Database at {}", db_url);

        let db = Database::new(&db_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize database: {}", e))?;

        let candle_repo = Arc::new(SqliteCandleRepository::new(db.pool.clone()));

        // 3. Initialize Infrastructure Services (Using Factory)
        let (market_service, execution_service, spread_cache) = ServiceFactory::create_services(
            &config,
            Some(candle_repo.clone()),
            portfolio.clone(),
        );

        // 4. Initialize remaining Persistence repositories
        let order_repo = Arc::new(SqliteOrderRepository::new(db.pool.clone()));
        let strategy_repo = Arc::new(SqliteStrategyRepository::new(db.pool.clone()));

        // 4. Initialize Adaptive Optimization Repositories
        let opt_history_repo = Arc::new(SqliteOptimizationHistoryRepository::new(db.pool.clone()));
        let snapshot_repo = Arc::new(SqlitePerformanceSnapshotRepository::new(db.pool.clone()));
        let trigger_repo = Arc::new(SqliteReoptimizationTriggerRepository::new(db.pool.clone()));

        // 5. Initialize Adaptive Optimization Services
        let performance_monitor = if config.adaptive_optimization_enabled {
            Some(Arc::new(PerformanceMonitoringService::new(
                snapshot_repo.clone(),
                candle_repo.clone(),
                market_service.clone(),
                portfolio.clone(),
                order_repo.clone(),
                config.regime_detection_window,
            )))
        } else {
            None
        };

        let adaptive_optimization_service = if config.adaptive_optimization_enabled {
            let es_clone = execution_service.clone();
            let execution_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync> =
                Arc::new(move || es_clone.clone());

            let optimizer = Arc::new(GridSearchOptimizer::new(
                market_service.clone(),
                execution_factory,
                ParameterGrid::default(), // Load from file in real world
                config.strategy_mode,
                config.min_profit_ratio, // Use config value
            ));

            Some(Arc::new(AdaptiveOptimizationService::new(
                optimizer,
                opt_history_repo,
                snapshot_repo,
                trigger_repo,
                strategy_repo.clone(),
                candle_repo.clone(),
                PerformanceEvaluator::new(EvaluationThresholds::default()),
                config.regime_detection_window,
                true,
            )))
        } else {
            None
        };

        // Log Risk Appetite configuration
        if let Some(ref appetite) = config.risk_appetite {
            info!(
                "Risk Appetite Score: {} ({:?}) - Calculated Parameters: risk_per_trade={:.2}%, trailing_stop={:.1}x, rsi_threshold={:.0}, max_position={:.1}%",
                appetite.score(),
                appetite.profile(),
                config.risk_per_trade_percent * 100.0,
                config.trailing_stop_atr_multiplier,
                config.rsi_threshold,
                config.max_position_size_pct * 100.0
            );
        }

        Ok(Self {
            config,
            market_service,
            execution_service,
            portfolio,
            order_repository: order_repo,
            candle_repository: Some(candle_repo),
            strategy_repository: strategy_repo,
            adaptive_optimization_service,
            performance_monitor,
            spread_cache,
        })
    }

    pub async fn start(self) -> Result<SystemHandle> {
        info!("Starting Agents...");

        // Initial Portfolio Sync
        info!("Synchronizing Portfolio State...");
        match self.execution_service.get_portfolio().await {
            Ok(initial_portfolio) => {
                let mut pf = self.portfolio.write().await;
                *pf = initial_portfolio;
                info!(
                    "Portfolio synchronized. Cash: ${}, Positions: {}",
                    pf.cash,
                    pf.positions.len()
                );
            }
            Err(e) => {
                warn!(
                    "Failed to fetch initial portfolio state: {}. Using default/empty state.",
                    e
                );
            }
        }

        let _portfolio_handle = self.portfolio.clone();

        let (market_tx, market_rx) = mpsc::channel(500); // High throughput: market data events
        let (proposal_tx, proposal_rx) = mpsc::channel(100); // Moderate: trade proposals
        let (order_tx, order_rx) = mpsc::channel(50); // Low throughput: approved orders
        let (throttled_order_tx, throttled_order_rx) = mpsc::channel(50); // Low throughput: throttled orders
        let (sentinel_cmd_tx, sentinel_cmd_rx) = mpsc::channel(10); // Very low: control commands
        let (risk_cmd_tx, risk_cmd_rx) = mpsc::channel(10); // Low: risk updates
        let (analyst_cmd_tx, analyst_cmd_rx) = mpsc::channel(10); // Low: config updates

        // Broadcast channel for Candles (for UI)
        let (candle_tx, candle_rx) = broadcast::channel(100);

        // Broadcast channel for Sentiment (for UI)
        let (sentiment_broadcast_tx, sentiment_broadcast_rx) = broadcast::channel(1);

        // Use the shared SpreadCache from Application (populated by WebSocket for Alpaca mode)
        let spread_cache = self.spread_cache.clone();

        // Create clones of Arc services for each task
        let market_service_for_sentinel = self.market_service.clone();
        let market_service_for_scanner = self.market_service.clone();
        let execution_service_for_scanner = self.execution_service.clone();

        let market_service_for_analyst = self.market_service.clone();
        let execution_service_for_analyst = self.execution_service.clone();
        let strategy_repo_for_analyst = self.strategy_repository.clone();
        let _order_repo_for_analyst = self.order_repository.clone();
        // win_rate_provider needs manual creation below

        let execution_service_for_state_manager = self.execution_service.clone();

        let execution_service_for_risk = self.execution_service.clone();
        let market_service_for_risk = self.market_service.clone();

        let execution_service_for_executor = self.execution_service.clone();
        let order_repo_for_executor = self.order_repository.clone();

        let candle_repo = self.candle_repository.clone(); // Option<Arc<..>> impls Clone

        let correlation_service = candle_repo.as_ref().map(|repo| Arc::new(CorrelationService::new(repo.clone())));

        // Return handle BEFORE moving self members, so we clone what we need.
        let system_handle = SystemHandle {
            sentinel_cmd_tx: sentinel_cmd_tx.clone(),
            risk_cmd_tx: risk_cmd_tx.clone(),
            analyst_cmd_tx: analyst_cmd_tx.clone(),
            proposal_tx: proposal_tx.clone(),
            portfolio: self.portfolio.clone(),
            candle_rx, // Move the receiver to the handle
            sentiment_rx: sentiment_broadcast_rx, // Move the receiver to the handle
            strategy_mode: self.config.strategy_mode,
            risk_appetite: self.config.risk_appetite,
        };

        // Now use self members
        let mut sentinel = Sentinel::new(
            market_service_for_sentinel,
            market_tx,
            self.config.symbols.clone(),
            Some(sentinel_cmd_rx),
        );

        let scanner_interval =
            std::time::Duration::from_secs(self.config.dynamic_scan_interval_minutes * 60);
        let scanner = MarketScanner::new(
            market_service_for_scanner,
            execution_service_for_scanner,
            sentinel_cmd_tx, // Use original tx
            scanner_interval,
            self.config.dynamic_symbol_mode,
        );

        let analyst_config = AnalystConfig {
            fast_sma_period: self.config.fast_sma_period,
            slow_sma_period: self.config.slow_sma_period,
            max_positions: self.config.max_positions,
            trade_quantity: self.config.trade_quantity,
            sma_threshold: self.config.sma_threshold,
            order_cooldown_seconds: self.config.order_cooldown_seconds,
            risk_per_trade_percent: self.config.risk_per_trade_percent,
            strategy_mode: self.config.strategy_mode,
            trend_sma_period: self.config.trend_sma_period,
            rsi_period: self.config.rsi_period,
            macd_fast_period: self.config.macd_fast_period,
            macd_slow_period: self.config.macd_slow_period,
            macd_signal_period: self.config.macd_signal_period,
            trend_divergence_threshold: self.config.trend_divergence_threshold,
            trailing_stop_atr_multiplier: self.config.trailing_stop_atr_multiplier,
            atr_period: self.config.atr_period,
            rsi_threshold: self.config.rsi_threshold,
            trend_riding_exit_buffer_pct: 0.03,
            mean_reversion_rsi_exit: self.config.mean_reversion_rsi_exit,
            mean_reversion_bb_period: self.config.mean_reversion_bb_period,
            slippage_pct: self.config.slippage_pct,
            commission_per_share: self.config.commission_per_share,
            max_position_size_pct: self.config.max_position_size_pct,
            bb_period: self.config.mean_reversion_bb_period,
            bb_std_dev: 2.0,
            macd_fast: self.config.macd_fast_period,
            macd_slow: self.config.macd_slow_period,
            macd_signal: self.config.macd_signal_period,
            ema_fast_period: self.config.ema_fast_period,
            ema_slow_period: self.config.ema_slow_period,
            take_profit_pct: self.config.take_profit_pct,
            min_hold_time_minutes: self.config.min_hold_time_minutes,
            signal_confirmation_bars: self.config.signal_confirmation_bars,
            spread_bps: self.config.spread_bps,
            min_profit_ratio: self.config.min_profit_ratio,
            macd_requires_rising: self.config.macd_requires_rising,
            trend_tolerance_pct: self.config.trend_tolerance_pct,
            macd_min_threshold: self.config.macd_min_threshold,
            profit_target_multiplier: self.config.profit_target_multiplier,
            adx_period: self.config.adx_period,
            adx_threshold: self.config.adx_threshold,
        };

        let strategy: Arc<dyn TradingStrategy> = match self.config.strategy_mode {
            crate::domain::market::strategy_config::StrategyMode::Standard => {
                Arc::new(DualSMAStrategy::new(
                    self.config.fast_sma_period,
                    self.config.slow_sma_period,
                    self.config.sma_threshold,
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
                    // Risk-appetite adaptive parameters
                    signal_confirmation_bars: analyst_config.signal_confirmation_bars,
                    macd_requires_rising: analyst_config.macd_requires_rising,
                    trend_tolerance_pct: analyst_config.trend_tolerance_pct,
                    macd_min_threshold: analyst_config.macd_min_threshold,
                    adx_threshold: analyst_config.adx_threshold,
                }))
            }
            crate::domain::market::strategy_config::StrategyMode::TrendRiding => {
                Arc::new(TrendRidingStrategy::new(
                    self.config.fast_sma_period,
                    self.config.slow_sma_period,
                    self.config.sma_threshold,
                    self.config.trend_riding_exit_buffer_pct,
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
        };

        let win_rate_provider = Arc::new(HistoricalWinRateProvider::new(
            self.order_repository.clone(),
            0.50, // Default conservative win rate
            10,   // Minimum 10 trades to switch to empirical data
        ));

        let mut analyst = Analyst::new(
            market_rx,
            analyst_cmd_rx,
            proposal_tx,
            analyst_config,
            strategy,
            AnalystDependencies {
                execution_service: execution_service_for_analyst,
                market_service: market_service_for_analyst,
                candle_repository: candle_repo,
                strategy_repository: Some(strategy_repo_for_analyst),
                win_rate_provider: Some(win_rate_provider),
                ui_candle_tx: Some(candle_tx),
                spread_cache: spread_cache.clone(),
            },
        );

        let sector_provider: Option<Arc<dyn SectorProvider>> = match self.config.mode {
            Mode::Alpaca => Some(Arc::new(AlpacaSectorProvider::new(
                self.config.alpaca_api_key.clone(),
                self.config.alpaca_secret_key.clone(),
                self.config.alpaca_base_url.clone(),
            ))),
            Mode::Mock => None,
            Mode::Oanda => Some(Arc::new(OandaSectorProvider)),
            Mode::Binance => Some(Arc::new(BinanceSectorProvider)),
        };

        let risk_config = crate::application::risk_management::risk_manager::RiskConfig {
            max_position_size_pct: self.config.max_position_size_pct,
            max_daily_loss_pct: self.config.max_daily_loss_pct,
            max_drawdown_pct: self.config.max_drawdown_pct,
            consecutive_loss_limit: self.config.consecutive_loss_limit,
            valuation_interval_seconds: 60,
            max_sector_exposure_pct: self.config.max_sector_exposure_pct,
            sector_provider,
            pending_order_ttl_ms: self.config.pending_order_ttl_ms,
            allow_pdt_risk: false, // Safer default
            correlation_config: crate::domain::risk::filters::correlation_filter::CorrelationFilterConfig {
                max_correlation_threshold: 0.85, // Default threshold
            },
        };

        // Create portfolio state manager for versioned state access
        let portfolio_state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                execution_service_for_state_manager,
                self.config
                    .portfolio_staleness_ms
                    .try_into()
                    .unwrap_or(5000), // Configurable staleness
            ),
        );

        let mut risk_manager = RiskManager::new(
            proposal_rx,
            risk_cmd_rx,
            order_tx,
            execution_service_for_risk,
            market_service_for_risk,
            portfolio_state_manager,
            self.config.non_pdt_mode,
            self.config.asset_class,
            risk_config,
            self.performance_monitor.clone(),
            correlation_service,
        );

        let mut order_throttler = OrderThrottler::new(
            order_rx,
            throttled_order_tx,
            self.config.max_orders_per_minute,
        );

        let mut executor = Executor::new(
            execution_service_for_executor,
            throttled_order_rx,
            self.portfolio.clone(),
            Some(order_repo_for_executor),
        );

        // Spawn Service Tasks
        tokio::spawn(async move { sentinel.run().await });
        tokio::spawn(async move { scanner.run().await });
        tokio::spawn(async move { analyst.run().await });
        tokio::spawn(async move { risk_manager.run().await });
        tokio::spawn(async move { order_throttler.run().await });
        tokio::spawn(async move { executor.run().await });

        // Spawn Sentiment Polling Task
        let sentiment_tx = risk_cmd_tx.clone();
        let asset_class = self.config.asset_class;
        tokio::spawn(async move {
            // Only poll for Crypto for now as we use Alternative.me
            // In future we can add VIX for stocks
            if asset_class == crate::config::AssetClass::Crypto {
                info!("Starting Sentiment Polling Task (Alternative.me)...");
                let provider = AlternativeMeSentimentProvider::new();
                
                // Initial fetch
                if let Ok(sentiment) = provider.fetch_sentiment().await {
                    let _ = sentiment_tx.send(RiskCommand::UpdateSentiment(sentiment.clone())).await;
                    let _ = sentiment_broadcast_tx.send(sentiment);
                }

                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(4 * 3600)).await; // Every 4 hours
                    match provider.fetch_sentiment().await {
                        Ok(sentiment) => {
                            if let Err(e) = sentiment_tx.send(RiskCommand::UpdateSentiment(sentiment.clone())).await {
                                error!("Failed to send sentiment update: {}", e);
                            }
                             let _ = sentiment_broadcast_tx.send(sentiment);
                        }
                        Err(e) => {
                            warn!("Failed to fetch sentiment: {}", e);
                        }
                    }
                }
            }
        });

        // Spawn Adaptive Optimization Task
        let adaptive_service = self.adaptive_optimization_service.clone();
        let symbols = self.config.symbols.clone();
        let eval_hour = self.config.adaptive_evaluation_hour;

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

        Ok(system_handle)
    }
}
