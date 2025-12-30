use anyhow::Result;
use chrono::Timelike;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

use crate::application::strategies::TradingStrategy;
use crate::application::{
    agents::{
        analyst::{Analyst, AnalystConfig},
        executor::Executor,
        scanner::MarketScanner,
        sentinel::Sentinel,
    },
    monitoring::performance_monitoring_service::PerformanceMonitoringService,
    optimization::{
        adaptive_optimization_service::AdaptiveOptimizationService,
        optimizer::{GridSearchOptimizer, ParameterGrid},
    },
    risk_management::{order_throttler::OrderThrottler, risk_manager::RiskManager},
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
use crate::infrastructure::alpaca::{
    AlpacaExecutionService, AlpacaMarketDataService, AlpacaSectorProvider,
};
use crate::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use crate::infrastructure::oanda::{
    OandaExecutionService, OandaMarketDataService, OandaSectorProvider,
};
use crate::infrastructure::persistence::database::Database;
use crate::infrastructure::persistence::repositories::{
    SqliteCandleRepository, SqliteOptimizationHistoryRepository, SqliteOrderRepository,
    SqlitePerformanceSnapshotRepository, SqliteReoptimizationTriggerRepository,
    SqliteStrategyRepository,
};

pub struct Application {
    pub config: Config,
    pub market_service: Arc<dyn MarketDataService>,
    pub execution_service: Arc<dyn ExecutionService>,
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub order_repository: Arc<dyn TradeRepository>,
    pub candle_repository: Arc<dyn CandleRepository>,
    pub strategy_repository: Arc<dyn StrategyRepository>,
    pub adaptive_optimization_service: Option<Arc<AdaptiveOptimizationService>>,
    pub performance_monitor: Option<Arc<PerformanceMonitoringService>>,
}

impl Application {
    pub async fn build(config: Config) -> Result<Self> {
        info!("Building Rustrade Application (Mode: {:?})...", config.mode);

        // 1. Initialize Shared State
        let mut initial_portfolio = Portfolio::new();
        initial_portfolio.cash = config.initial_cash;
        let portfolio = Arc::new(RwLock::new(initial_portfolio));

        // 2. Initialize Infrastructure
        let (market_service, execution_service): (
            Arc<dyn MarketDataService>,
            Arc<dyn ExecutionService>,
        ) = match config.mode {
            Mode::Mock => {
                info!("Using Mock services");
                (
                    Arc::new(MockMarketDataService::new()),
                    Arc::new(MockExecutionService::new(portfolio.clone())),
                )
            }
            Mode::Alpaca => {
                info!("Using Alpaca services ({})", config.alpaca_base_url);
                (
                    Arc::new(AlpacaMarketDataService::new(
                        config.alpaca_api_key.clone(),
                        config.alpaca_secret_key.clone(),
                        config.alpaca_ws_url.clone(),
                        config.alpaca_data_url.clone(),
                        config.min_volume_threshold,
                    )),
                    Arc::new(AlpacaExecutionService::new(
                        config.alpaca_api_key.clone(),
                        config.alpaca_secret_key.clone(),
                        config.alpaca_base_url.clone(),
                    )),
                )
            }
            Mode::Oanda => {
                info!("Using OANDA services ({})", config.oanda_api_base_url);
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
                )
            }
        };

        // 3. Initialize Persistence
        let db_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://rustrade.db".to_string());
        info!("Initializing Database at {}", db_url);

        let db = Database::new(&db_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize database: {}", e))?;

        let order_repo = Arc::new(SqliteOrderRepository::new(db.pool.clone()));
        let candle_repo = Arc::new(SqliteCandleRepository::new(db.pool.clone()));
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
            candle_repository: candle_repo,
            strategy_repository: strategy_repo,
            adaptive_optimization_service,
            performance_monitor,
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting Agents...");

        let (market_tx, market_rx) = mpsc::channel(100);
        let (proposal_tx, proposal_rx) = mpsc::channel(100);
        let (order_tx, order_rx) = mpsc::channel(100);
        let (throttled_order_tx, throttled_order_rx) = mpsc::channel(100);
        let (sentinel_cmd_tx, sentinel_cmd_rx) = mpsc::channel(100);

        let mut sentinel = Sentinel::new(
            self.market_service.clone(),
            market_tx,
            self.config.symbols.clone(),
            Some(sentinel_cmd_rx),
        );

        let scanner_interval =
            std::time::Duration::from_secs(self.config.dynamic_scan_interval_minutes * 60);
        let scanner = MarketScanner::new(
            self.market_service.clone(),
            self.execution_service.clone(),
            sentinel_cmd_tx,
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
        };

        let strategy: Arc<dyn TradingStrategy> = match self.config.strategy_mode {
            crate::domain::market::strategy_config::StrategyMode::Standard => {
                Arc::new(DualSMAStrategy::new(
                    self.config.fast_sma_period,
                    self.config.slow_sma_period,
                    self.config.sma_threshold,
                ))
            }
            crate::domain::market::strategy_config::StrategyMode::Advanced => {
                Arc::new(AdvancedTripleFilterStrategy::new(
                    self.config.fast_sma_period,
                    self.config.slow_sma_period,
                    self.config.sma_threshold,
                    self.config.trend_sma_period,
                    self.config.rsi_threshold,
                ))
            }
            crate::domain::market::strategy_config::StrategyMode::Dynamic => {
                Arc::new(DynamicRegimeStrategy::new(
                    self.config.fast_sma_period,
                    self.config.slow_sma_period,
                    self.config.sma_threshold,
                    self.config.trend_sma_period,
                    self.config.rsi_threshold,
                    self.config.trend_divergence_threshold,
                ))
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
                    self.config.mean_reversion_bb_period,
                    self.config.mean_reversion_rsi_exit,
                ))
            }
        };

        let mut analyst = Analyst::new(
            market_rx,
            proposal_tx,
            self.execution_service.clone(),
            strategy,
            analyst_config,
            Some(self.candle_repository.clone()),
            Some(self.strategy_repository.clone()),
        );

        let sector_provider: Option<Arc<dyn SectorProvider>> = match self.config.mode {
            Mode::Alpaca => Some(Arc::new(AlpacaSectorProvider::new(
                self.config.alpaca_api_key.clone(),
                self.config.alpaca_secret_key.clone(),
                self.config.alpaca_base_url.clone(),
            ))),
            Mode::Mock => None,
            Mode::Oanda => Some(Arc::new(OandaSectorProvider)),
        };

        let risk_config = crate::application::risk_management::risk_manager::RiskConfig {
            max_position_size_pct: self.config.max_position_size_pct,
            max_daily_loss_pct: self.config.max_daily_loss_pct,
            max_drawdown_pct: self.config.max_drawdown_pct,
            consecutive_loss_limit: self.config.consecutive_loss_limit,
            valuation_interval_seconds: 60,
            max_sector_exposure_pct: self.config.max_sector_exposure_pct,
            sector_provider,
        };

        let mut risk_manager = RiskManager::new(
            proposal_rx,
            order_tx,
            self.execution_service.clone(),
            self.market_service.clone(),
            self.portfolio.clone(),
            self.config.non_pdt_mode,
            self.config.asset_class,
            risk_config,
            self.performance_monitor.clone(),
        );

        let mut order_throttler = OrderThrottler::new(
            order_rx,
            throttled_order_tx,
            self.config.max_orders_per_minute,
        );

        let mut executor = Executor::new(
            self.execution_service.clone(),
            throttled_order_rx,
            self.portfolio.clone(),
            Some(self.order_repository.clone()),
        );

        // Spawn Service Tasks
        let sentinel_handle = tokio::spawn(async move { sentinel.run().await });
        let scanner_handle = tokio::spawn(async move { scanner.run().await });
        let analyst_handle = tokio::spawn(async move { analyst.run().await });
        let risk_manager_handle = tokio::spawn(async move { risk_manager.run().await });
        let throttler_handle = tokio::spawn(async move { order_throttler.run().await });
        let executor_handle = tokio::spawn(async move { executor.run().await });

        // Spawn Adaptive Optimization Task
        let adaptive_service = self.adaptive_optimization_service.clone();
        let symbols = self.config.symbols.clone();
        let eval_hour = self.config.adaptive_evaluation_hour;

        let adaptive_handle = tokio::spawn(async move {
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

        let _ = tokio::join!(
            sentinel_handle,
            analyst_handle,
            risk_manager_handle,
            throttler_handle,
            executor_handle,
            scanner_handle,
            adaptive_handle
        );

        Ok(())
    }
}
