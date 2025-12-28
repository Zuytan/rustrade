use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::info;

use crate::application::{
    analyst::{Analyst, AnalystConfig},
    executor::Executor,
    order_throttler::OrderThrottler,
    risk_manager::RiskManager,
    scanner::MarketScanner,
    sentinel::Sentinel,
    strategies::*,
};
use crate::config::{Config, Mode};
use crate::domain::portfolio::Portfolio;
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::repositories::{CandleRepository, TradeRepository};
use crate::infrastructure::alpaca::{AlpacaExecutionService, AlpacaMarketDataService};
use crate::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use crate::infrastructure::persistence::database::Database;
use crate::infrastructure::persistence::repositories::{
    SqliteCandleRepository, SqliteOrderRepository,
};

pub struct Application {
    pub config: Config,
    pub market_service: Arc<dyn MarketDataService>,
    pub execution_service: Arc<dyn ExecutionService>,
    pub portfolio: Arc<RwLock<Portfolio>>,
    pub order_repository: Option<Arc<dyn TradeRepository>>,
    pub candle_repository: Option<Arc<dyn CandleRepository>>,
}

impl Application {
    pub async fn build(config: Config) -> Result<Self> {
        // Setup Logging if not already set (optional, or handle in main)
        // For tests we might want to suppress or redirect logs, but for now we'll assume main handles it
        // or we do it here if it's the first time.
        // A common pattern is to let the binary handle global logging setup.

        info!("Building Rustrade Application (Mode: {:?})...", config.mode);

        // Initialize Shared State
        let mut initial_portfolio = Portfolio::new();
        initial_portfolio.cash = config.initial_cash;
        let portfolio = Arc::new(RwLock::new(initial_portfolio));

        // Initialize Infrastructure
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
                    )),
                    Arc::new(AlpacaExecutionService::new(
                        config.alpaca_api_key.clone(),
                        config.alpaca_secret_key.clone(),
                        config.alpaca_base_url.clone(),
                    )),
                )
            }
        };

        // Initialize Persistence
        let db_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://rustrade.db".to_string());
        info!("Initializing Database at {}", db_url);

        let db = Database::new(&db_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize database: {}", e))?;

        let order_repo: Arc<dyn TradeRepository> =
            Arc::new(SqliteOrderRepository::new(db.pool.clone()));
        let candle_repo: Arc<dyn CandleRepository> =
            Arc::new(SqliteCandleRepository::new(db.pool.clone()));

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
        } else {
            info!("Using individual risk parameters from environment");
        }

        Ok(Self {
            config,
            market_service,
            execution_service,
            portfolio,
            order_repository: Some(order_repo),
            candle_repository: Some(candle_repo),
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

        // Scan internal in seconds for simpler config mapping if needed, or keep duration
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
            max_position_size_pct: self.config.max_position_size_pct,
        };

        // Create strategy based on config
        let strategy: Arc<dyn TradingStrategy> = match self.config.strategy_mode {
            crate::config::StrategyMode::Standard => Arc::new(DualSMAStrategy::new(
                self.config.fast_sma_period,
                self.config.slow_sma_period,
                self.config.sma_threshold,
            )),
            crate::config::StrategyMode::Advanced => Arc::new(AdvancedTripleFilterStrategy::new(
                self.config.fast_sma_period,
                self.config.slow_sma_period,
                self.config.sma_threshold,
                self.config.trend_sma_period,
                self.config.rsi_threshold,
            )),
            crate::config::StrategyMode::Dynamic => Arc::new(DynamicRegimeStrategy::new(
                self.config.fast_sma_period,
                self.config.slow_sma_period,
                self.config.sma_threshold,
                self.config.trend_sma_period,
                self.config.rsi_threshold,
                self.config.trend_divergence_threshold,
            )),
            crate::config::StrategyMode::TrendRiding => Arc::new(TrendRidingStrategy::new(
                self.config.fast_sma_period,
                self.config.slow_sma_period,
                self.config.sma_threshold,
                self.config.trend_riding_exit_buffer_pct,
            )),
            crate::config::StrategyMode::MeanReversion => Arc::new(MeanReversionStrategy::new(
                self.config.mean_reversion_bb_period,
                self.config.mean_reversion_rsi_exit,
            )),
        };

        info!("Using strategy: {}", strategy.name());
        let mut analyst = Analyst::new(
            market_rx,
            proposal_tx,
            self.execution_service.clone(),
            strategy,
            analyst_config,
            self.candle_repository.clone(),
        );

        let mut risk_manager = RiskManager::new(
            proposal_rx,
            order_tx,
            self.execution_service.clone(),
            self.market_service.clone(),
            self.config.non_pdt_mode,
            crate::application::risk_manager::RiskConfig {
                max_position_size_pct: self.config.max_position_size_pct,
                max_daily_loss_pct: self.config.max_daily_loss_pct,
                max_drawdown_pct: self.config.max_drawdown_pct,
                consecutive_loss_limit: self.config.consecutive_loss_limit,
            },
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
            self.order_repository.clone(),
        );

        // Spawn Tasks
        let sentinel_handle = tokio::spawn(async move {
            sentinel.run().await;
        });

        let scanner_handle = tokio::spawn(async move {
            scanner.run().await;
        });

        let analyst_handle = tokio::spawn(async move {
            analyst.run().await;
        });

        let risk_manager_handle = tokio::spawn(async move {
            risk_manager.run().await;
        });

        let throttler_handle = tokio::spawn(async move {
            order_throttler.run().await;
        });

        let executor_handle = tokio::spawn(async move {
            executor.run().await;
        });

        let _ = tokio::join!(
            sentinel_handle,
            analyst_handle,
            risk_manager_handle,
            throttler_handle,
            executor_handle,
            scanner_handle
        );

        Ok(())
    }
}
