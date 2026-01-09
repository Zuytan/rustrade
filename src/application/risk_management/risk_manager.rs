use crate::domain::ports::{ExecutionService, MarketDataService, OrderUpdate, SectorProvider};
use crate::domain::repositories::RiskStateRepository;
use crate::domain::risk::state::RiskState;
use crate::domain::sentiment::Sentiment;
#[cfg(test)]
use crate::domain::sentiment::SentimentClassification;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Order, OrderSide, OrderStatus, TradeProposal};
use crate::application::risk_management::pipeline::validation_pipeline::RiskValidationPipeline;
use crate::application::risk_management::state::risk_state_manager::RiskStateManager;
use crate::application::risk_management::state::pending_orders_tracker::PendingOrdersTracker;
use crate::application::risk_management::session_manager::SessionManager;
use crate::application::risk_management::portfolio_valuation_service::PortfolioValuationService;
use crate::application::risk_management::liquidation_service::LiquidationService;
use crate::domain::risk::filters::{
    RiskValidator, ValidationContext, ValidationResult,
    position_size_validator::{PositionSizeValidator, PositionSizeConfig},
    circuit_breaker_validator::{CircuitBreakerValidator, CircuitBreakerConfig},
    pdt_validator::{PdtValidator, PdtConfig},
    sector_exposure_validator::{SectorExposureValidator, SectorExposureConfig},
    sentiment_validator::{SentimentValidator, SentimentConfig},
    correlation_filter::{CorrelationFilter, CorrelationFilterConfig},
    buying_power_validator::{BuyingPowerValidator, BuyingPowerConfig},
};
use crate::domain::risk::volatility_manager::{VolatilityConfig, VolatilityManager}; // Added
use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock; // Added
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::application::monitoring::correlation_service::CorrelationService;
use crate::application::monitoring::performance_monitoring_service::PerformanceMonitoringService;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::commands::RiskCommand;
use crate::config::AssetClass;

/// Error type for RiskManager configuration validation
#[derive(Debug, thiserror::Error)]
pub enum RiskConfigError {
    #[error("Invalid RiskConfig: {0}")]
    ValidationError(String),
}

/// Risk management configuration
#[derive(Clone)]
pub struct RiskConfig {
    pub max_position_size_pct: f64, // Max % of equity per position (e.g., 0.25 = 25%)
    pub max_daily_loss_pct: f64,    // Max % loss per day (e.g., 0.02 = 2%)
    pub max_drawdown_pct: f64,      // Max % drawdown from high water mark (e.g., 0.10 = 10%)
    pub consecutive_loss_limit: usize, // Max consecutive losing trades before halt
    pub valuation_interval_seconds: u64, // Interval for portfolio valuation check
    pub max_sector_exposure_pct: f64, // Max exposure per sector
    pub sector_provider: Option<Arc<dyn SectorProvider>>,
    pub allow_pdt_risk: bool, // If true, allows opening orders even if PDT saturated (Risky!)
    pub pending_order_ttl_ms: Option<i64>, // TTL for pending orders filled but not synced
    pub correlation_config: CorrelationFilterConfig,
    pub volatility_config: VolatilityConfig, // Added
}

impl std::fmt::Debug for RiskConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RiskConfig")
            .field("max_position_size_pct", &self.max_position_size_pct)
            .field("max_daily_loss_pct", &self.max_daily_loss_pct)
            .field("max_drawdown_pct", &self.max_drawdown_pct)
            .field("consecutive_loss_limit", &self.consecutive_loss_limit)
            .field(
                "valuation_interval_seconds",
                &self.valuation_interval_seconds,
            )
            .field("max_sector_exposure_pct", &self.max_sector_exposure_pct)
            .field("allow_pdt_risk", &self.allow_pdt_risk)
            .field("pending_order_ttl_ms", &self.pending_order_ttl_ms)
            .field("correlation_config", &self.correlation_config)
            .field("volatility_config", &self.volatility_config)
            .finish()
    }
}

impl RiskConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_position_size_pct <= 0.0 || self.max_position_size_pct > 1.0 {
            return Err(format!(
                "Invalid max_position_size_pct: {}",
                self.max_position_size_pct
            ));
        }
        if self.max_daily_loss_pct <= 0.0 || self.max_daily_loss_pct > 0.5 {
            return Err(format!(
                "Invalid max_daily_loss_pct: {}",
                self.max_daily_loss_pct
            ));
        }
        if self.max_drawdown_pct <= 0.0 || self.max_drawdown_pct > 1.0 {
            return Err(format!(
                "Invalid max_drawdown_pct: {}",
                self.max_drawdown_pct
            ));
        }
        if self.consecutive_loss_limit == 0 {
            return Err("consecutive_loss_limit must be > 0".to_string());
        }
        if self.max_sector_exposure_pct <= 0.0 || self.max_sector_exposure_pct > 1.0 {
            return Err(format!(
                "Invalid max_sector_exposure_pct: {}",
                self.max_sector_exposure_pct
            ));
        }
        Ok(())
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_size_pct: 0.10, // Reduced from 0.25 for safety
            max_daily_loss_pct: 0.02,    // 2%
            max_drawdown_pct: 0.05,      // Reduced from 0.10 for safety
            consecutive_loss_limit: 3,
            valuation_interval_seconds: 60,
            max_sector_exposure_pct: 0.20, // Reduced from 0.30

            sector_provider: None,
            allow_pdt_risk: false,
            pending_order_ttl_ms: None, // Default 5 mins
            correlation_config: CorrelationFilterConfig::default(),
            volatility_config: VolatilityConfig::default(),
        }
    }
}

pub struct RiskManager {
    proposal_rx: Receiver<TradeProposal>,
    external_cmd_rx: Receiver<RiskCommand>,
    order_tx: Sender<Order>,
    execution_service: Arc<dyn ExecutionService>,
    market_service: Arc<dyn MarketDataService>,
    portfolio_state_manager: Arc<PortfolioStateManager>,
    performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    correlation_service: Option<Arc<CorrelationService>>,
    risk_config: RiskConfig,
    volatility_manager: Arc<RwLock<VolatilityManager>>,

    asset_class: AssetClass,

    // NEW Architecture Components
    validation_pipeline: RiskValidationPipeline,
    #[allow(dead_code)]
    state_manager: RiskStateManager,
    #[allow(dead_code)]
    pending_orders_tracker: PendingOrdersTracker,
    
    // Extracted Services
    session_manager: SessionManager,
    portfolio_valuation_service: PortfolioValuationService,
    liquidation_service: LiquidationService,

    // Legacy State (Deprecated/To be removed) 
    // Kept briefly if needed for transition, but aim to remove usage
    risk_state: RiskState, // REPLACED BY state_manager
    pending_orders: HashMap<String, PendingOrder>, // REPLACED BY pending_orders_tracker

    // Runtime flags
    halted: bool,
    daily_pnl: Decimal, 

    // Cache
    current_prices: HashMap<String, Decimal>,
    pending_reservations:
        HashMap<String, crate::application::monitoring::portfolio_state_manager::ReservationToken>, // Exposure reservations
    current_sentiment: Option<Sentiment>,
    risk_state_repository: Option<Arc<dyn RiskStateRepository>>,
}

#[derive(Debug, Clone)]
struct PendingOrder {
    symbol: String,
    side: OrderSide,
    requested_qty: Decimal,
    filled_qty: Decimal,
    filled_but_not_synced: bool, // Track filled orders awaiting portfolio confirmation
    entry_price: Decimal,        // Track for P&L calculation on sell
    filled_at: Option<i64>,      // Timestamp when filled (for TTL cleanup)
}

impl PendingOrder {
}

impl RiskManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proposal_rx: Receiver<TradeProposal>,
        external_cmd_rx: Receiver<RiskCommand>,
        order_tx: Sender<Order>,
        execution_service: Arc<dyn ExecutionService>,
        market_service: Arc<dyn MarketDataService>,
        portfolio_state_manager: Arc<PortfolioStateManager>,
        non_pdt_mode: bool,
        asset_class: AssetClass,
        risk_config: RiskConfig,
        performance_monitor: Option<Arc<PerformanceMonitoringService>>,
        correlation_service: Option<Arc<CorrelationService>>,
        risk_state_repository: Option<Arc<dyn RiskStateRepository>>,
    ) -> Result<Self, RiskConfigError> {
        // Validate configuration
        risk_config
            .validate()
            .map_err(RiskConfigError::ValidationError)?;

        // --- Build Validation Pipeline ---
        // --- Build Validation Pipeline ---
        let validators: Vec<Box<dyn RiskValidator>> = vec![
            // 1. Top Priority: Circuit Breaker
            Box::new(CircuitBreakerValidator::new(CircuitBreakerConfig {
                max_daily_loss_pct: risk_config.max_daily_loss_pct,
                max_drawdown_pct: risk_config.max_drawdown_pct,
                consecutive_loss_limit: risk_config.consecutive_loss_limit,
            })),

            // 2. Regulatory: PDT
            Box::new(PdtValidator::new(PdtConfig {
                enabled: !non_pdt_mode && !risk_config.allow_pdt_risk, 
                asset_class,
                ..Default::default()
            })),

            // 3. Diversification: Sector Exposure
            Box::new(SectorExposureValidator::new(SectorExposureConfig {
                max_sector_exposure_pct: risk_config.max_sector_exposure_pct,
                sector_provider: risk_config.sector_provider.clone(),
            })),

            // 4. Diversification: Correlation
            Box::new(CorrelationFilter::new(
                 risk_config.correlation_config.clone()
            )),

            // 5. Risk Sizing: Position Size
            Box::new(PositionSizeValidator::new(PositionSizeConfig {
                max_position_size_pct: risk_config.max_position_size_pct,
            })),

            // 6. Optimization: Sentiment
            Box::new(SentimentValidator::new(SentimentConfig::default())),

            // 7. Affordability: Buying Power (Available Cash)
            Box::new(BuyingPowerValidator::new(BuyingPowerConfig::default())),
        ];

        let validation_pipeline = RiskValidationPipeline::new(validators);

        // --- State Management ---
        let state_manager = RiskStateManager::new(
            risk_state_repository.clone(),
            Decimal::ZERO, // Initialized later in initialize_session
        );

        let pending_orders_tracker = PendingOrdersTracker::new();
        let volatility_manager = Arc::new(RwLock::new(VolatilityManager::new(
            risk_config.volatility_config.clone(),
        )));
        
        // Initialize extracted services
        let session_manager = SessionManager::new(
            risk_state_repository.clone(),
            market_service.clone(),
        );
        
        let portfolio_valuation_service = PortfolioValuationService::new(
            market_service.clone(),
            portfolio_state_manager.clone(),
            volatility_manager.clone(),
            asset_class,
        );
        
        let liquidation_service = LiquidationService::new(
            order_tx.clone(),
            portfolio_state_manager.clone(),
        );

        Ok(Self {
            proposal_rx,
            external_cmd_rx,
            order_tx,
            execution_service,
            market_service,
            portfolio_state_manager,

            asset_class,
            risk_config,
            volatility_manager,
            
            // New Components
            validation_pipeline,
            state_manager,
            pending_orders_tracker,
            
            // Extracted Services
            session_manager,
            portfolio_valuation_service,
            liquidation_service,

            // Legacy State (Initialized to defaults, will be synced or ignored)
            risk_state: RiskState::default(),
            pending_orders: HashMap::new(),
            
            current_prices: HashMap::new(),
            performance_monitor,
            correlation_service,

            halted: false,
            daily_pnl: Decimal::ZERO,
            
            pending_reservations: HashMap::new(),
            current_sentiment: None,
            risk_state_repository,
        })
    }

    /// Persist current risk state to database
    async fn persist_state(&self) {
        if let Some(repo) = &self.risk_state_repository {
            let state = RiskState {
                id: "global".to_string(), // Singleton for now
                session_start_equity: self.risk_state.session_start_equity,
                daily_start_equity: self.risk_state.daily_start_equity,
                equity_high_water_mark: self.risk_state.equity_high_water_mark,
                consecutive_losses: self.risk_state.consecutive_losses,
                reference_date: self.risk_state.reference_date,
                updated_at: Utc::now().timestamp(),
                daily_drawdown_reset: false, // Default or track it
            };
            
            if let Err(e) = repo.save(&state).await {
                error!("RiskManager: Failed to persist risk state: {}", e);
            } else {
                debug!("RiskManager: Risk state persisted successfully.");
            }
        }
    }

    /// Initialize session tracking with starting equity
    /// Delegates to SessionManager for session lifecycle management
    pub async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Get portfolio snapshot
        let snapshot = self.portfolio_state_manager.refresh().await?;
        
        // Delegate session initialization to SessionManager
        let risk_state = self.session_manager
            .initialize_session(&snapshot.portfolio, &mut self.current_prices)
            .await?;
        
        // Sync to legacy state (will be removed in future)
        self.risk_state = risk_state;
        
        info!(
            "RiskManager: Session initialized. Equity: {}, Daily Start: {}, HWM: {}",
            self.risk_state.session_start_equity, 
            self.risk_state.daily_start_equity, 
            self.risk_state.equity_high_water_mark
        );

        // SYNC Fix: Ensure state_manager is also updated with the initialized state
        // otherwise update_portfolio_valuation will overwrite self.risk_state with empty state
        *self.state_manager.get_state_mut() = self.risk_state.clone();
        
        Ok(())
    }



    /// Check if circuit breaker should trigger
    fn check_circuit_breaker(&self, current_equity: Decimal) -> Option<String> {
        // Check daily loss limit
        if self.risk_state.session_start_equity > Decimal::ZERO {
            let daily_loss_pct = ((current_equity - self.risk_state.session_start_equity)
                / self.risk_state.session_start_equity)
                .to_f64()
                .unwrap_or(0.0);

            if daily_loss_pct < -self.risk_config.max_daily_loss_pct {
                let msg = format!(
                    "Daily loss limit breached: {:.2}% (limit: {:.2}%) [Start: {}, Current: {}]",
                    daily_loss_pct * 100.0,
                    self.risk_config.max_daily_loss_pct * 100.0,
                    self.risk_state.session_start_equity,
                    current_equity
                );
                return Some(msg);
            }
        }

        // Check drawdown limit
        if self.risk_state.equity_high_water_mark > Decimal::ZERO {
            let drawdown_pct = ((current_equity - self.risk_state.equity_high_water_mark)
                / self.risk_state.equity_high_water_mark)
                .to_f64()
                .unwrap_or(0.0);

            if drawdown_pct < -self.risk_config.max_drawdown_pct {
                return Some(format!(
                    "Max drawdown breached: {:.2}% (limit: {:.2}%)",
                    drawdown_pct * 100.0,
                    self.risk_config.max_drawdown_pct * 100.0
                ));
            }
        }

        // Check consecutive losses
        if self.risk_state.consecutive_losses >= self.risk_config.consecutive_loss_limit {
            return Some(format!(
                "Consecutive loss limit reached: {} trades (limit: {})",
                self.risk_state.consecutive_losses, self.risk_config.consecutive_loss_limit
            ));
        }

        None
    }

    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    fn handle_order_update(&mut self, update: OrderUpdate) -> bool {
        // info!("RiskManager: processing update for order {}", update.order_id);

        // If we don't have the order in pending, we might have started tracking after it was sent?
        // Or it's from another session. We can only track what we know.
        // Try both order_id and client_order_id potentially if we tracked by COID?
        // But we tracked by Order.id (which is usually UUID we generated).
        // Alpaca returns their ID in order_id?
        // Wait. `Order` struct has `id`. We set it to UUID.
        // `AlpacaExecutionService` maps `order.id` (our UUID) to `client_order_id` in Alpaca.
        // The `OrderUpdate` from stream has `client_order_id`.
        // So we should match on `client_order_id` (our UUID) OR `order_id` (Alpaca ID)?
        // RiskManager keys `pending_orders` by the UUID it generated.
        // `OrderUpdate.client_order_id` SHOULD match that UUID.

        let mut state_changed = false;

        if let Some(pending) = self.pending_orders.get_mut(&update.client_order_id) {
            match update.status {
                OrderStatus::Filled | OrderStatus::PartiallyFilled => {
                    pending.filled_qty = update.filled_qty;
                    if pending.filled_qty >= pending.requested_qty {
                        // Full fill: Mark as tentative instead of removing
                        // Keep order in pending until REST portfolio confirms position
                        // This prevents "phantom position" race condition where:
                        // 1. WebSocket confirms fill
                        // 2. Portfolio REST API not yet updated
                        // 3. Next signal sees 0 exposure and double-allocates
                        pending.filled_but_not_synced = true;
                        pending.filled_at = Some(chrono::Utc::now().timestamp_millis());

                        // Track P&L for SELL orders to update consecutive loss counter
                        if pending.side == OrderSide::Sell
                            && let Some(fill_price) = update.filled_avg_price {
                                let pnl = (fill_price - pending.entry_price) * pending.filled_qty;
                                if pnl < Decimal::ZERO {
                                    self.state_manager.get_state_mut().consecutive_losses += 1;
                                    warn!(
                                        "RiskManager: Trade LOSS detected for {} (${:.2}). Consecutive losses: {}",
                                        pending.symbol, pnl, self.state_manager.get_state().consecutive_losses
                                    );
                                    state_changed = true;
                                } else {
                                    self.state_manager.get_state_mut().consecutive_losses = 0;
                                    state_changed = true; // Resetting is also a change we want to persist
                                    info!(
                                        "RiskManager: Trade PROFIT for {} (${:.2}). Loss streak reset.",
                                        pending.symbol, pnl
                                    );
                                }
                            }

                        info!(
                            "RiskManager: Order {} FILLED (tentative) - awaiting portfolio sync for {}",
                            &update.client_order_id[..8], pending.symbol
                        );
                    }
                }
                OrderStatus::Cancelled
                | OrderStatus::Rejected
                | OrderStatus::Expired
                | OrderStatus::Suspended => {
                    // Terminal states that don't result in positions: remove immediately
                }
                _ => {}
            }

            // Cleanup only non-fill terminal states (Cancelled/Rejected/Expired)
            // Filled orders stay in pending until portfolio confirms
            if matches!(
                update.status,
                OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired
            ) {
                self.pending_orders.remove(&update.client_order_id);

                // Release any exposure reservation
                if let Some(token) = self.pending_reservations.remove(&update.client_order_id) {
                    let state_manager = self.portfolio_state_manager.clone();
                    tokio::spawn(async move {
                        state_manager.release_reservation(token).await;
                    });
                }
            }
        }
        
        state_changed
    }



    /// Fetch latest prices for all held positions and update valuation
    /// Delegates to PortfolioValuationService for valuation updates
    pub async fn update_portfolio_valuation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Delegate valuation to PortfolioValuationService
        let (_portfolio, current_equity) = self.portfolio_valuation_service
            .update_portfolio_valuation(&mut self.current_prices)
            .await?;
        
        // Update volatility
        let _ = self.portfolio_valuation_service.update_volatility().await;
        
        // Update High Water Mark via State Manager
        self.state_manager.update(current_equity, Utc::now()).await;
        // Sync legacy copy
        self.risk_state = self.state_manager.get_state().clone();
        
        // Check Risks (Async check)
        // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
        if !self.halted
            && let Some(reason) = self.check_circuit_breaker(current_equity) {
                tracing::error!("RiskManager MONITOR: CIRCUIT BREAKER TRIGGERED: {}", reason);
                self.halted = true;
                self.liquidate_portfolio(&reason).await;
            }
        
        // Capture performance snapshot if monitor available
        if let Some(monitor) = &self.performance_monitor {
            for sym in self.current_prices.keys() {
                let _ = monitor.capture_snapshot(sym).await;
            }
        }
        
        Ok(())
    }

    /// Update volatility manager with latest ATR/Benchmark data
    pub async fn update_volatility(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Choose benchmark symbol based on asset class
        let benchmark = match self.asset_class {
            AssetClass::Crypto => "BTC/USDT",
            _ => "SPY",
        };

        // Fetch last 20 daily candles for ATR
        let now = Utc::now();
        let start = now - chrono::Duration::days(30); // 30 days to get enough candles

        match self.market_service
            .get_historical_bars(benchmark, start, now, "1D")
            .await 
        {
            Ok(candles) => {
                if candles.len() < 2 {
                    return Ok(());
                }

                // Calculate a simple True Range for the latest candle
                // TR = Max(H-L, H-Cp, L-Cp)
                // For simplicity here, we take H-L
                let last = &candles[candles.len() - 1];
                let high = last.high.to_f64().unwrap_or(0.0);
                let low = last.low.to_f64().unwrap_or(0.0);
                let range = high - low;

                if range > 0.0 {
                    let mut vm = self.volatility_manager.write().await;
                    vm.update(range);
                    debug!("RiskManager: Volatility updated for {}. Latest range: {:.2}, Avg: {:.2}", 
                        benchmark, range, vm.get_average_volatility());
                }
            }
            Err(e) => {
                warn!("RiskManager: Failed to fetch volatility data: {}", e);
            }
        }

        Ok(())
    }

    /// Emergency liquidation of entire portfolio
    /// Delegates to LiquidationService for emergency liquidation logic
    async fn liquidate_portfolio(&mut self, reason: &str) {
        // Delegate liquidation to LiquidationService
        self.liquidation_service
            .liquidate_portfolio(reason, &self.current_prices)
            .await;
    }

    /// Check if we need to reset session stats (for 24/7 Crypto markets)
    fn check_daily_reset(&mut self, current_equity: Decimal) -> bool {
        // Delegate to RiskStateManager
        self.state_manager.check_daily_reset(current_equity);
        
        // Sync local legacy state
        let old_reset = self.risk_state.daily_drawdown_reset;
        self.risk_state = self.state_manager.get_state().clone();
        
        if self.risk_state.daily_drawdown_reset && !old_reset {
             self.daily_pnl = Decimal::ZERO;
             return true;
        }
        
        // If reference date changed, it was a reset
        let today = Utc::now().date_naive();
        if self.asset_class == AssetClass::Crypto && self.risk_state.reference_date == today {
             // It might have been updated by manager just now.
             // But we need to know if it CHANGED just now. 
             // We can assume state_manager handles it.
             // Returning false is safe if manager handles persistence (via update called elsewhere).
             // But caller expects bool to call persist.
             
             // Simplification: always return false here and rely on update_portfolio_valuation to persist state changes?
             // But daily reset happens on proposal too.
             
             // Check if updated_at is recent?
             if self.risk_state.updated_at >= Utc::now().timestamp() - 1 {
                 return true;
             }
        }
        false
    }

    /// Cleanup tentative filled orders and release reservations
    fn reconcile_pending_orders(&mut self, portfolio: &Portfolio) {
        let ttl_ms = self.risk_config.pending_order_ttl_ms.unwrap_or(300_000);

        // We need to capture pending_reservations and portfolio_state_manager for the closure
        // But we can't capture `self` fully if we borrow `pending_orders`.
        // However, since `pending_reservations` is a separate field, split borrowing works.
        // We might need to clone the manager outside or access safely.

        let pending_reservations = &mut self.pending_reservations;
        let state_manager = self.portfolio_state_manager.clone();

        self.pending_orders.retain(|order_id, pending| {
            if pending.filled_but_not_synced {
                // Check TTL for stuck orders - cleanup if older than TTL
                if let Some(filled_at) = pending.filled_at {
                    let age_ms = chrono::Utc::now().timestamp_millis() - filled_at;
                    if age_ms > ttl_ms {
                        warn!(
                            "RiskManager: Pending order {} TTL expired after {}ms. Forcing cleanup for {}",
                            &order_id[..8], age_ms, pending.symbol
                        );
                        // Release reservation if exists
                        if let Some(token) = pending_reservations.remove(order_id) {
                            let mgr = state_manager.clone();
                            tokio::spawn(async move { mgr.release_reservation(token).await; });
                        }
                        return false; // Remove from pending
                    }
                }

                // Check if position exists in portfolio for this symbol
                let normalized_symbol = pending.symbol.replace("/", "").replace(" ", "");
                let in_portfolio = portfolio.positions.iter().any(|(sym, pos)| {
                    let normalized_sym = sym.replace("/", "").replace(" ", "");
                    normalized_sym == normalized_symbol && pos.quantity > Decimal::ZERO
                });

                if in_portfolio {
                    // Portfolio synced! Remove pending and release reservation
                    info!(
                        "RiskManager: Reconciled order {} - {} now confirmed in portfolio",
                        &order_id[..8], pending.symbol
                    );

                    if let Some(token) = pending_reservations.remove(order_id) {
                        let mgr = state_manager.clone();
                        tokio::spawn(async move {
                            mgr.release_reservation(token).await;
                        });
                    }
                    return false; // Remove from pending
                }
            }
            true // Keep in pending
        });
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    // ============================================================================
    // COMMAND PATTERN HANDLERS
    // ============================================================================

    /// Dispatch command to appropriate handler
    async fn handle_command(&mut self, cmd: RiskCommand) -> Result<(), Box<dyn std::error::Error>> {
        match cmd {
            RiskCommand::OrderUpdate(update) => self.cmd_handle_order_update(update).await,
            RiskCommand::ValuationTick => self.cmd_handle_valuation().await,
            RiskCommand::RefreshPortfolio => self.cmd_handle_refresh().await,
            RiskCommand::ProcessProposal(proposal) => self.cmd_handle_proposal(proposal).await,
            RiskCommand::UpdateSentiment(sentiment) => self.cmd_handle_update_sentiment(sentiment).await,
            RiskCommand::UpdateConfig(config) => self.cmd_handle_update_config(config).await,
            RiskCommand::CircuitBreakerTrigger => {
                warn!("RiskManager: MANUAL CIRCUIT BREAKER TRIGGERED! Executing Panic Liquidation.");
                self.liquidate_portfolio("Manual Circuit Breaker Trigger").await;
                Ok(())
            },
        }
    }

    async fn cmd_handle_update_config(&mut self, config: Box<RiskConfig>) -> Result<(), Box<dyn std::error::Error>> {
        info!("RiskManager: Updating risk configuration: {:?}", config);
        self.risk_config = *config;
        Ok(())
    }
    
    async fn cmd_handle_update_sentiment(&mut self, sentiment: Sentiment) -> Result<(), Box<dyn std::error::Error>> {
        info!("RiskManager: Received Market Sentiment: {} ({})", sentiment.value, sentiment.classification);
        self.current_sentiment = Some(sentiment);
        Ok(())
    }

    /// Handle portfolio refresh command
    async fn cmd_handle_refresh(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.portfolio_state_manager.refresh().await?;
        Ok(())
    }

    /// Handle order update command
    async fn cmd_handle_order_update(&mut self, update: OrderUpdate) -> Result<(), Box<dyn std::error::Error>> {
        if self.handle_order_update(update) {
             self.persist_state().await; 
        }
        Ok(())
    }

    /// Handle valuation tick command
    async fn cmd_handle_valuation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.update_portfolio_valuation().await?;

        if !self.halted {
            let snapshot = self.portfolio_state_manager.get_snapshot().await;
            if self.check_daily_reset(snapshot.portfolio.total_equity(&self.current_prices)) {
                self.persist_state().await;
            }
            self.reconcile_pending_orders(&snapshot.portfolio);
        }

        Ok(())
    }

    /// Handle trade proposal command
    async fn cmd_handle_proposal(&mut self, proposal: TradeProposal) -> Result<(), Box<dyn std::error::Error>> {
        if self.halted {
            warn!("RiskManager: Trading HALTED. Rejecting proposal for {}", proposal.symbol);
            return Ok(());
        }

        debug!("RiskManager: reviewing proposal {:?}", proposal);

        // Update current price
        self.current_prices.insert(proposal.symbol.clone(), proposal.price);

        // Get portfolio snapshot
        let mut snapshot = self.portfolio_state_manager.get_snapshot().await;

        // Refresh if stale
        if self.portfolio_state_manager.is_stale(&snapshot) {
            snapshot = self.portfolio_state_manager.refresh().await?;
        }

        // Reconcile pending orders
        self.reconcile_pending_orders(&snapshot.portfolio);

        // Calculate current equity
        let current_equity = snapshot.portfolio.total_equity(&self.current_prices);

        // Update high water mark
        if current_equity > self.risk_state.equity_high_water_mark {
            self.risk_state.equity_high_water_mark = current_equity;
        }

        // Check daily reset
        if self.check_daily_reset(current_equity) {
             self.persist_state().await;
        }

        // Circuit breaker check (Trigger Liquidation logic)
        // Keeps the system safe by checking global health before processing specific trade rules
        if let Some(reason) = self.check_circuit_breaker(current_equity) {
            error!("RiskManager: CIRCUIT BREAKER TRIGGERED - {}", reason);
            self.halted = true;
            self.liquidate_portfolio(&reason).await;
            return Ok(());
        }

        // Prepare Validation Context
        let correlation_matrix = if let Some(service) = &self.correlation_service {
             // Pre-fetch correlation matrix if service available
             // Optimization: We could let the validator ask for it, but context is passive.
             // We get existing symbols + proposal symbol
             let mut symbols: Vec<String> = snapshot.portfolio.positions.keys().cloned().collect();
             if !symbols.contains(&proposal.symbol) {
                 symbols.push(proposal.symbol.clone());
             }
             service.calculate_correlation_matrix(&symbols).await.ok()
        } else {
             None
        };

        let volatility_multiplier = {
            let vm = self.volatility_manager.read().await;
            // For now we use the average multiplier if no specific current vol is fed
            // Or we could have a "get_current_multiplier" that uses a default or last known.
            // Let's assume we want to pass a value here.
            // If we don't have current volatility data, we use 1.0.
            Some(vm.calculate_multiplier(vm.get_average_volatility()))
        };

        let pending_exposure = self.pending_orders.values()
            .filter(|p| p.symbol == proposal.symbol && p.side == OrderSide::Buy)
            .fold(Decimal::ZERO, |acc, p| acc + (p.requested_qty * p.entry_price));

        let ctx = ValidationContext::new(
            &proposal,
            &snapshot.portfolio,
            current_equity,
            &self.current_prices,
            &self.risk_state,
            self.current_sentiment.as_ref(),
            correlation_matrix.as_ref(), // Pass pre-calculated matrix
            volatility_multiplier,
            pending_exposure,
            snapshot.available_cash(),
        );

        // Execute Pipeline
        match self.validation_pipeline.validate(&ctx).await {
            ValidationResult::Approve => {
                // All checks passed
                self.execute_proposal_internal(proposal, &snapshot.portfolio).await?;
            }
            ValidationResult::Reject(reason) => {
                warn!(
                    "RiskManager: Rejecting {:?} order for {} - {}",
                    proposal.side, proposal.symbol, reason
                );
            }
        }

        Ok(())
    }

    /// Internal proposal execution logic (extracted from run())
    async fn execute_proposal_internal(
        &mut self,
        proposal: TradeProposal,
        _portfolio: &Portfolio,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create order with correct structure
        let order = Order {
            id: Uuid::new_v4().to_string(),
            symbol: proposal.symbol.clone(),
            side: proposal.side,
            price: proposal.price,
            quantity: proposal.quantity,
            order_type: proposal.order_type,
            timestamp: Utc::now().timestamp_millis(),
        };

        // Track as pending
        self.pending_orders.insert(
            order.id.clone(),
            PendingOrder {
                symbol: proposal.symbol.clone(),
                side: proposal.side,
                requested_qty: proposal.quantity,
                filled_qty: Decimal::ZERO,
                filled_but_not_synced: false,
                entry_price: proposal.price,
                filled_at: None,
            },
        );

        // Submit order
        info!(
            "RiskManager: Submitting {:?} order for {} qty {} @ {}",
            proposal.side, proposal.symbol, proposal.quantity, proposal.price
        );

        if let Err(e) = self.order_tx.send(order.clone()).await {
            error!("RiskManager: Failed to send order: {}", e);
            self.pending_orders.remove(&order.id);
            return Err(Box::new(e));
        }

        Ok(())
    }

    pub async fn run(&mut self) {
        info!("RiskManager started with config: {:?}", self.risk_config);

        // Initialize session
        if let Err(e) = self.initialize_session().await {
            error!("RiskManager: Failed to initialize session: {}", e);
        }

        // Ticker for periodic valuation
        let mut valuation_interval = tokio::time::interval(tokio::time::Duration::from_secs(
            self.risk_config.valuation_interval_seconds,
        ));

        // Ticker for periodic portfolio refresh (uses config from PortfolioStateManager)
        // Default: refresh every 2 seconds to keep snapshot fresh
        let refresh_interval_ms = std::env::var("PORTFOLIO_REFRESH_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(2000);
        let mut refresh_interval =
            tokio::time::interval(tokio::time::Duration::from_millis(refresh_interval_ms));

        // Subscribe to Real-Time Order Updates
        let mut order_update_rx = match self.execution_service.subscribe_order_updates().await {
            Ok(rx) => Some(rx),
            Err(e) => {
                error!("RiskManager: Failed to subscribe to order updates: {}. Pending tracking will be limited.", e);
                None
            }
        };

        // Ticker for periodic volatility update
        let mut vol_interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // Every hour

        loop {
            tokio::select! {
                // Periodic volatility refresh
                _ = vol_interval.tick() => {
                    if let Err(e) = self.update_volatility().await {
                        error!("RiskManager: Volatility update failed: {}", e);
                    }
                }

                // Periodic portfolio state refresh
                _ = refresh_interval.tick() => {
                    if let Err(e) = self.handle_command(RiskCommand::RefreshPortfolio).await {
                        error!("RiskManager: Portfolio refresh failed: {}", e);
                    }
                }
                
                // Listen for Order Updates (handle lag explicitly)
                result = async {
                    if let Some(rx) = &mut order_update_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    match result {
                        Ok(update) => {
                            if let Err(e) = self.handle_command(RiskCommand::OrderUpdate(update)).await {
                                error!("RiskManager: Order update handling failed: {}", e);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(
                                "RiskManager: Order update receiver lagged, missed {} updates! Forcing refresh.",
                                n
                            );
                            if let Err(e) = self.handle_command(RiskCommand::RefreshPortfolio).await {
                                error!("RiskManager: Failed to refresh after lag: {}", e);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            error!("RiskManager: Order update channel closed! Real-time tracking disabled.");
                            order_update_rx = None;
                        }
                    }
                }
                
                // Periodic valuation
                _ = valuation_interval.tick() => {
                    if let Err(e) = self.handle_command(RiskCommand::ValuationTick).await {
                        error!("RiskManager: Valuation failed: {}", e);
                    }
                }
                
                // Process trade proposals
                Some(proposal) = self.proposal_rx.recv() => {
                    if let Err(e) = self.handle_command(RiskCommand::ProcessProposal(proposal)).await {
                        error!("RiskManager: Proposal processing failed: {}", e);
                    }
                }

                // External commands (Sentiment, etc.)
                Some(cmd) = self.external_cmd_rx.recv() => {
                    if let Err(e) = self.handle_command(cmd).await {
                        error!("RiskManager: External command processing failed: {}", e);
                    }
                }
            }
        }
    }
}




#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AssetClass;
    use crate::domain::trading::portfolio::{Portfolio, Position};
    use crate::domain::trading::types::{OrderSide, OrderType};
    use crate::infrastructure::mock::{MockExecutionService, MockMarketDataService};
    use chrono::Utc;
    
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromPrimitive;
    use tokio::sync::{mpsc, RwLock};

    use std::sync::Mutex;

    struct ConfigurableMockMarketData {
        prices: Arc<Mutex<HashMap<String, Decimal>>>,
    }

    impl ConfigurableMockMarketData {
        fn new() -> Self {
            Self {
                prices: Arc::new(Mutex::new(HashMap::new())),
            }
        }
        fn set_price(&self, symbol: &str, price: Decimal) {
            let mut prices = self.prices.lock().unwrap();
            prices.insert(symbol.to_string(), price);
        }
    }

    #[async_trait::async_trait]
    impl MarketDataService for ConfigurableMockMarketData {
        async fn subscribe(
            &self,
            _symbols: Vec<String>,
        ) -> Result<mpsc::Receiver<crate::domain::trading::types::MarketEvent>, anyhow::Error>
        {
            let (_, rx) = mpsc::channel(1);
            Ok(rx)
        }
        async fn get_top_movers(&self) -> Result<Vec<String>, anyhow::Error> {
            Ok(vec![])
        }
        async fn get_prices(
            &self,
            symbols: Vec<String>,
        ) -> Result<HashMap<String, Decimal>, anyhow::Error> {
            let prices = self.prices.lock().unwrap();
            let mut result = HashMap::new();
            for sym in symbols {
                if let Some(p) = prices.get(&sym) {
                    result.insert(sym, *p);
                }
            }
            Ok(result)
        }
        async fn get_historical_bars(
            &self,
            _symbol: &str,
            _start: chrono::DateTime<chrono::Utc>,
            _end: chrono::DateTime<chrono::Utc>,
            _timeframe: &str,
        ) -> Result<Vec<crate::domain::trading::types::Candle>, anyhow::Error> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_on_market_crash() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);

        // Setup Portfolio: $10,000 Cash + 100 TSLA @ $100 ($10,000 Value) = $20,000 Equity
        let mut port = Portfolio::new();
        port.cash = Decimal::from(10000);
        port.positions.insert(
            "TSLA".to_string(),
            Position {
                symbol: "TSLA".to_string(),
                quantity: Decimal::from(100),
                average_price: Decimal::from(100),
            },
        );
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

        // Setup Market: TSLA @ $100 Initially
        let market_data = Arc::new(ConfigurableMockMarketData::new());
        market_data.set_price("TSLA", Decimal::from(100));
        let market_service = market_data.clone();

        // Config: Max Daily Loss 5%
        let config = RiskConfig {
            max_daily_loss_pct: 0.10,
            valuation_interval_seconds: 1,
            correlation_config: CorrelationFilterConfig::default(),
            ..RiskConfig::default()
        };

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Stock,
            config,
            None,
            None,
            None,
        )
        .expect("Test config should be valid");

        // Run RiskManager in background
        tokio::spawn(async move { rm.run().await });

        // Wait for initialization (should set session start equity to $20,000)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // CRASH MARKET: TSLA -> $80 (-20%)
        // New Equity: $10k + $8k = $18k. Loss = $2k (10%). Should trigger 5% limit.
        market_data.set_price("TSLA", Decimal::from(80));

        // Wait for RiskManager ticker (we set it to 60s in code... WAIT)
        // The ticker is hardcoded to 60s in `RiskManager::run`.
        // Ideally we should make it configurable or use a mocked time, but for this integration test:
        // We can't wait 60s.
        // Option 1: Change RiskManager to accept ticker interval config.
        // Option 2: Send a proposal! The proposal loop ALSO updates valuation.

        let proposal = TradeProposal {
            symbol: "TSLA".to_string(),
            side: OrderSide::Buy, // Buy more?
            price: Decimal::from(80),
            quantity: Decimal::from(10),
            order_type: OrderType::Market,
            reason: "Buy the dip".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal).await.unwrap();

        // Expect Liquidation Order due to Circuit Breaker
        let liquidation_order =
            tokio::time::timeout(std::time::Duration::from_millis(200), order_rx.recv())
                .await
                .expect("Should trigger liquidation")
                .expect("Should receive liquidation order");

        assert_eq!(liquidation_order.symbol, "TSLA");
        assert_eq!(liquidation_order.side, OrderSide::Sell);
        assert_eq!(liquidation_order.order_type, OrderType::Market); // Emergency liquidation uses Market

        // Ensure NO other orders (like the proposal) are processed
        assert!(
            order_rx.try_recv().is_err(),
            "Should catch only liquidation order"
        );

        // Note: verifying logs is hard here, but rejection confirms logic.
    }

    #[tokio::test]
    async fn test_buy_approval() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
        let market_service = Arc::new(MockMarketDataService::new());

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Stock,
            RiskConfig::default(),
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(1),
            order_type: OrderType::Market,
            reason: "Test".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal).await.unwrap();

        let order = order_rx.recv().await.expect("Should approve");
        assert_eq!(order.symbol, "ABC");
    }

    #[tokio::test]
    async fn test_buy_rejection_insufficient_funds() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(50); // Less than 100
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
        let market_service = Arc::new(MockMarketDataService::new());

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Stock,
            RiskConfig::default(),
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(1),
            order_type: OrderType::Market,
            reason: "Test".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal).await.unwrap();

        // Give it a moment to process (or fail to process)
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(order_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_buy_rejection_insufficient_buying_power_high_equity() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000); 
        // High equity via positions
        port.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: Decimal::from(1000),
                average_price: Decimal::from(100),
            },
        );
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
        
        // Mock Market Data (Need AAPL price for Equity calc)
        let market_data = Arc::new(ConfigurableMockMarketData::new());
        market_data.set_price("AAPL", Decimal::from(100)); // $100k Equity
        let market_service = market_data;

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        // Default config: 10% max position size = $10,000 (approx 10% of $101,000 equity)
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service.clone(), 
            state_manager,
            false,
            AssetClass::Stock,
            RiskConfig::default(),
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        
        // Ensure price map is populated in RM
        // RiskManager refreshes prices on run loop.
        
        tokio::spawn(async move { rm.run().await });

        // Initialize session
        // rm.initialize_session().await.unwrap(); // Called by run() automatically

        // Proposal: Buy $5,000
        // Position Size check: $5,000 < $10,100 (10% of equity). PASS.
        // Buying Power check: $5,000 > $1,000 (Available Cash). REJECT.
        let proposal = TradeProposal {
            symbol: "MSFT".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(50), // $5,000
            order_type: OrderType::Market,
            reason: "Test Buying Power".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal).await.unwrap();

        // Give it a moment to process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        
        // Assert NO order was generated
        assert!(order_rx.try_recv().is_err(), "Order should be rejected due to insufficient buying power despite high equity");
    }

    #[tokio::test]
    async fn test_sell_approval() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.positions.insert(
            "ABC".to_string(),
            Position {
                symbol: "ABC".to_string(),
                quantity: Decimal::from(10), // Own 10
                average_price: Decimal::from(50),
            },
        );
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
        let market_service = Arc::new(MockMarketDataService::new());

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Stock,
            RiskConfig::default(),
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Sell,
            price: Decimal::from(100),
            quantity: Decimal::from(5), // Sell 5
            order_type: OrderType::Market,
            reason: "Test".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal).await.unwrap();

        let order = order_rx.recv().await.expect("Should approve");
        assert_eq!(order.symbol, "ABC");
    }

    #[tokio::test]
    async fn test_pdt_protection_rejection() {
        let (_proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(20000); // Trigger is_pdt_risk
        port.day_trades_count = 3; // Trigger pdt saturation
        port.positions.insert(
            "ABC".to_string(),
            Position {
                symbol: "ABC".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::from(50),
            },
        );
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

        // Simulate a BUY today
        exec_service
            .execute(Order {
                id: "buy1".to_string(),
                symbol: "ABC".to_string(),
                side: OrderSide::Buy,
                price: Decimal::from(50),
                quantity: Decimal::from(10),
                order_type: OrderType::Limit,
                timestamp: Utc::now().timestamp_millis(),
            })
            .await
            .unwrap();

        // New RiskManager with NON_PDT_MODE = true
        let market_service = Arc::new(MockMarketDataService::new());
        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );


        let risk_config = RiskConfig {
            max_daily_loss_pct: 0.5, // 50% max allowed
            max_drawdown_pct: 0.5, // 50%
            ..Default::default()
        };

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false, // non_pdt_mode = false (trigger protection)
            AssetClass::Stock,
            risk_config,
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        
        // Initialize state (this fetches initial portfolio and prices)
        rm.initialize_session().await.unwrap();

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Sell,
            price: Decimal::from(60),
            quantity: Decimal::from(5),
            order_type: OrderType::Market,
            reason: "Test PDT".to_string(),
            timestamp: Utc::now().timestamp_millis(),
        };
        
        // Handle command directly (via Command Pattern!)
        rm.handle_command(RiskCommand::ProcessProposal(proposal)).await.unwrap();

        // Should be REJECTED (no order sent to order_rx)
        assert!(order_rx.try_recv().is_err(), "Order should have been rejected by PDT protection but was sent!");
    }

    struct MockSectorProvider {
        sectors: HashMap<String, String>,
    }

    #[async_trait::async_trait]
    impl SectorProvider for MockSectorProvider {
        async fn get_sector(&self, symbol: &str) -> Result<String, anyhow::Error> {
            Ok(self
                .sectors
                .get(symbol)
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string()))
        }
    }

    #[tokio::test]
    async fn test_sector_exposure_limit() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);

        // Setup Portfolio: $100,000 Cash + $25,000 AAPL (Tech) = $125,000 Equity
        let mut port = Portfolio::new();
        port.cash = Decimal::from(100000);
        port.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: Decimal::from(100),
                average_price: Decimal::from(250),
            },
        );
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

        // Setup Market
        let market_data = Arc::new(ConfigurableMockMarketData::new());
        market_data.set_price("AAPL", Decimal::from(250));
        market_data.set_price("MSFT", Decimal::from(200));
        let market_service = market_data.clone();

        // Setup Sector Provider
        let mut sectors = HashMap::new();
        sectors.insert("AAPL".to_string(), "Tech".to_string());
        sectors.insert("MSFT".to_string(), "Tech".to_string());
        let sector_provider = Arc::new(MockSectorProvider { sectors });

        let config = RiskConfig {
            max_sector_exposure_pct: 0.30,
            sector_provider: Some(sector_provider),
            ..RiskConfig::default()
        };

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Stock,
            config,
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        tokio::spawn(async move { rm.run().await });

        // Proposal: Buy MSFT (Tech) $20,000
        // New Tech Exposure: $25,000 (AAPL) + $20,000 (MSFT) = $45,000
        // New Equity (approx): $125,000
        // Pct: 45,000 / 125,000 = 36% > 30% -> REJECT
        let proposal = TradeProposal {
            symbol: "MSFT".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(200),
            quantity: Decimal::from(100), // 100 * 200 = 20,000
            reason: "Sector Test".to_string(),
            timestamp: 0,
            order_type: OrderType::Market,
        };
        proposal_tx.send(proposal).await.unwrap();

        // Should be REJECTED
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            order_rx.try_recv().is_err(),
            "Should reject due to sector exposure"
        );
    }

    #[tokio::test]
    async fn test_circuit_breaker_triggers_liquidation() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(10); // Buffer for liquidation orders

        // Setup Portfolio: $10,000 Cash + 10 TSLA @ $1000 ($10,000 Value) = $20,000 Equity
        let mut port = Portfolio::new();
        port.cash = Decimal::from(10000);
        port.positions.insert(
            "TSLA".to_string(),
            Position {
                symbol: "TSLA".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::from(1000),
            },
        );
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));

        // Setup Market
        let market_data = Arc::new(ConfigurableMockMarketData::new());
        market_data.set_price("TSLA", Decimal::from(1000));
        let market_service = market_data.clone();

        // Config: Max Daily Loss 10% ($2,000)
        let config = RiskConfig {
            max_daily_loss_pct: 0.10,
            ..RiskConfig::default()
        };

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Stock,
            config,
            None,
            None,
            None,
        )
        .expect("Test config should be valid");

        tokio::spawn(async move { rm.run().await });

        // Initialize session (Equity = $20,000)
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // CRASH SCENARIO: TSLA Drops to $700 (-30%)
        // Equity drops from $20k to $17k (-15%). This exceeds 10% limit.
        // We trigger this by sending a proposal (which updates price cache)
        let proposal = TradeProposal {
            symbol: "TSLA".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(700),
            quantity: Decimal::from(1),
            order_type: OrderType::Market,
            reason: "Trying to catch a falling knife".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal).await.unwrap();

        // Expect:
        // 1. Proposal Rejected (implied by liquidation triggering)
        // 2. Liquidation Order for TSLA (Market Sell 10 units)

        let liquidation_order =
            tokio::time::timeout(std::time::Duration::from_millis(200), order_rx.recv())
                .await
                .expect("Should return liquidation order")
                .expect("Should have an order");

        assert_eq!(liquidation_order.symbol, "TSLA");
        assert_eq!(liquidation_order.side, OrderSide::Sell);
        assert_eq!(liquidation_order.quantity, Decimal::from(10));
        assert_eq!(liquidation_order.order_type, OrderType::Market); // Emergency liquidation uses Market

        // Verify subsequent proposals are rejected (Halted state)
        let proposal2 = TradeProposal {
            symbol: "AAPL".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(150),
            quantity: Decimal::from(1),
            order_type: OrderType::Market,
            reason: "Safe trade".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal2).await.unwrap();

        // Should receive NO orders
        let res =
            tokio::time::timeout(std::time::Duration::from_millis(100), order_rx.recv()).await;
        assert!(res.is_err(), "Should timeout because trading is halted");
    }

    #[tokio::test]
    async fn test_crypto_daily_reset() {
        // Test that session start equity resets when day changes for Crypto
        let (_proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, _order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(10000);
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
        let market_service = Arc::new(MockMarketDataService::new());
        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let (_, dummy_cmd_rx) = mpsc::channel(1);
        let mut rm = RiskManager::new(
            proposal_rx,
            dummy_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Crypto, // Enable Crypto mode
            RiskConfig::default(),
            None,
            None,
            None,
        )
        .expect("Test config should be valid");

        // Manually manipulate last_reset_date to yesterday in STATE MANAGER
        let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
        let yesterday_ts = (Utc::now() - chrono::Duration::days(1)).timestamp();
        
        rm.state_manager.get_state_mut().reference_date = yesterday;
        rm.state_manager.get_state_mut().updated_at = yesterday_ts;
        rm.state_manager.get_state_mut().session_start_equity = Decimal::from(5000); // Old baseline
        rm.state_manager.get_state_mut().daily_drawdown_reset = false;

        // Sync legacy state to avoid confusion (though logic depends on manager)
        rm.risk_state = rm.state_manager.get_state().clone();

        // Wait, current_equity argument needed.
        let current_equity = Decimal::from(10000);
        rm.check_daily_reset(current_equity);

        assert_eq!(
            rm.risk_state.session_start_equity, current_equity,
            "Should reset session equity to current"
        );
        assert_eq!(
            rm.risk_state.reference_date,
            Utc::now().date_naive(),
            "Should update reset date to today"
        );
    }

    #[tokio::test]
    async fn test_sentiment_risk_adjustment() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (risk_cmd_tx, risk_cmd_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);

        // Portfolio: $10,000 Cash
        let mut port = Portfolio::new();
        port.cash = Decimal::from(10000);
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
        let market_service = Arc::new(MockMarketDataService::new());

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let risk_config = RiskConfig {
            max_position_size_pct: 0.10, // 10% normally ($1000)
            ..Default::default()
        };

        let mut rm = RiskManager::new(
            proposal_rx,
            risk_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Crypto,
            risk_config,
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        tokio::spawn(async move { rm.run().await });

        // 1. Inject Sentiment: Extreme Fear (20)
        let sentiment = Sentiment {
            value: 20,
            classification: SentimentClassification::from_score(20),
            timestamp: Utc::now(),
            source: "Test".to_string(),
        };
        risk_cmd_tx.send(RiskCommand::UpdateSentiment(sentiment)).await.unwrap();
        
        // Wait for processing
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // 2. Proposal: Buy $600 worth (6%)
        // Normal Limit: $1000 (10%) -> Would Pass
        // Extreme Fear Limit: $500 (5%) -> Should Fail
        let proposal = TradeProposal {
            symbol: "BTC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(60000),
            quantity: Decimal::from_f64(0.01).unwrap(), // $600
            order_type: OrderType::Market,
            reason: "Test Sentiment".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal).await.unwrap();

        // 3. Verify Rejection
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(order_rx.try_recv().is_err(), "Should be rejected due to Sentiment adjustment");

        // 4. Inject Sentiment: Greed (60)
         let sentiment_greed = Sentiment {
            value: 60,
            classification: SentimentClassification::from_score(60),
            timestamp: Utc::now(),
            source: "Test".to_string(),
        };
        risk_cmd_tx.send(RiskCommand::UpdateSentiment(sentiment_greed)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // 5. Resend Proposal (Should Pass now, limit is back to 10%)
        let proposal2 = TradeProposal {
            symbol: "BTC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(60000),
            quantity: Decimal::from_f64(0.01).unwrap(), // $600 < $1000
            order_type: OrderType::Market,
            reason: "Test Sentiment Greed".to_string(),
            timestamp: 0,
        };
        proposal_tx.send(proposal2).await.unwrap();

        // 6. Verify Acceptance
        let order = order_rx.recv().await.expect("Should be approved in Greed mode");
        assert_eq!(order.symbol, "BTC");
    }

    #[tokio::test]
    async fn test_blind_liquidation_panic_mode() {
        // 1. Setup
        let portfolio = Portfolio::new();
        let portfolio = Arc::new(RwLock::new(portfolio));
        
        let exec_service = Arc::new(MockExecutionService::new(portfolio.clone()));
        // MockMarketDataService returns 0 if price not set
        let market_service = Arc::new(MockMarketDataService::new()); 
        
        let (_proposal_tx, proposal_rx) = mpsc::channel(1);
        let (risk_cmd_tx, risk_cmd_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);

        let risk_config = RiskConfig {
            max_daily_loss_pct: 0.5,
            ..Default::default()
        };

        // Portfolio has 10 BTC
        {
            let mut p = portfolio.write().await;
            p.cash = Decimal::from(1000);
            p.positions.insert("BTC".to_string(), crate::domain::trading::portfolio::Position {
                symbol: "BTC".to_string(),
                quantity: Decimal::from(10),
                average_price: Decimal::from(100),
            });
        }

        let state_manager = Arc::new(
            crate::application::monitoring::portfolio_state_manager::PortfolioStateManager::new(
                exec_service.clone(),
                5000,
            ),
        );

        let mut rm = RiskManager::new(
            proposal_rx,
            risk_cmd_rx,
            order_tx,
            exec_service,
            market_service,
            state_manager,
            false,
            AssetClass::Crypto,
            risk_config,
            None,
            None,
            None,
        )
        .expect("Test config should be valid");
        tokio::spawn(async move { rm.run().await });

        // 2. Trigger Liquidation (with 0 price)
        // We do NOT set price on MockMarketDataService, so it returns None or 0 depending on implementation.
        // Even if it returns 0, logic should proceed.
        
        // Wait for init
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        info!("Triggering Liquidation with NO PRICE data (Panic Mode)...");
        risk_cmd_tx.send(RiskCommand::CircuitBreakerTrigger).await.unwrap();

        // 3. Expect Market Sell Order
        let order = order_rx.recv().await.expect("Should receive liquidation order even without price");
        
        assert_eq!(order.symbol, "BTC");
        assert_eq!(order.side, OrderSide::Sell);
        assert_eq!(order.quantity, Decimal::from(10));
        assert!(matches!(order.order_type, OrderType::Market), "Must be Market order in panic mode");
    }
}
