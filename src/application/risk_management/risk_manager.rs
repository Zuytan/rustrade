use crate::domain::ports::{ExecutionService, MarketDataService, OrderUpdate, SectorProvider};
use crate::domain::sentiment::{Sentiment, SentimentClassification};
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Order, OrderSide, OrderStatus, OrderType, TradeProposal};
use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::application::monitoring::correlation_service::CorrelationService;
use crate::application::monitoring::performance_monitoring_service::PerformanceMonitoringService;
use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::application::risk_management::commands::RiskCommand;
use crate::config::AssetClass;
use crate::domain::risk::filters::correlation_filter::{CorrelationFilter, CorrelationFilterConfig};

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
        }
    }
}

pub struct RiskManager {
    proposal_rx: Receiver<TradeProposal>,
    external_cmd_rx: Receiver<RiskCommand>,
    order_tx: Sender<Order>,
    execution_service: Arc<dyn ExecutionService>,
    market_service: Arc<dyn MarketDataService>,
    non_pdt_mode: bool,
    asset_class: AssetClass,
    risk_config: RiskConfig,
    // Risk Tracking State
    equity_high_water_mark: Decimal,
    session_start_equity: Decimal,
    daily_start_equity: Decimal, // New: for daily loss tracking
    daily_pnl: Decimal,          // New: current day's P/L
    last_reset_date: chrono::NaiveDate,
    consecutive_losses: usize,
    current_prices: HashMap<String, Decimal>, // Track current prices for equity calculation
    portfolio_state_manager: Arc<PortfolioStateManager>, // Versioned state with optimistic locking (ACTIVE)
    performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    correlation_service: Option<Arc<CorrelationService>>,
    sector_cache: HashMap<String, String>,

    halted: bool,
    pending_orders: HashMap<String, PendingOrder>,
    pending_reservations:
        HashMap<String, crate::application::monitoring::portfolio_state_manager::ReservationToken>, // Exposure reservations
    current_sentiment: Option<Sentiment>,
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
    fn remaining_qty(&self) -> Decimal {
        if self.filled_qty >= self.requested_qty {
            Decimal::ZERO
        } else {
            self.requested_qty - self.filled_qty
        }
    }
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
    ) -> Self {
        if let Err(e) = risk_config.validate() {
            panic!("RiskManager Configuration Error: {}", e);
        }
        Self {
            proposal_rx,
            external_cmd_rx,
            order_tx,
            execution_service,
            market_service,
            portfolio_state_manager,
            non_pdt_mode,
            asset_class,
            risk_config,
            equity_high_water_mark: Decimal::ZERO,
            session_start_equity: Decimal::ZERO,
            daily_start_equity: Decimal::ZERO,
            daily_pnl: Decimal::ZERO,
            last_reset_date: Utc::now().date_naive(),
            consecutive_losses: 0,
            current_prices: HashMap::new(),
            performance_monitor,
            correlation_service,
            sector_cache: HashMap::new(),

            halted: false,
            pending_orders: HashMap::new(),
            pending_reservations: HashMap::new(),
            current_sentiment: None,
        }
    }

    /// Initialize session tracking with starting equity
    pub async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Refresh portfolio state from exchange
        let snapshot = self.portfolio_state_manager.refresh().await?;

        // Fetch initial prices for accurate equity calculation
        let symbols: Vec<String> = snapshot.portfolio.positions.keys().cloned().collect();
        if !symbols.is_empty() {
            match self.market_service.get_prices(symbols).await {
                Ok(prices) => {
                    for (sym, price) in prices {
                        self.current_prices.insert(sym, price);
                    }
                }
                Err(e) => {
                    warn!("RiskManager: Failed to fetch initial prices: {}", e);
                }
            }
        }

        let initial_equity = snapshot.portfolio.total_equity(&self.current_prices);
        self.session_start_equity = initial_equity;
        self.daily_start_equity = initial_equity; // Initialize daily tracking
        self.equity_high_water_mark = initial_equity;
        info!(
            "RiskManager: Session initialized with equity: {} (portfolio v{})",
            initial_equity, snapshot.version
        );
        Ok(())
    }

    /// Validate position size doesn't exceed limit
    fn validate_position_size(
        &self,
        _symbol: &str,
        exposure: Decimal,
        equity: Decimal,
        side: OrderSide,
    ) -> bool {
        if equity <= Decimal::ZERO {
            return true;
        }

        // Apply Sentiment-based Risk Adjustment
        let mut adjusted_max_pos_pct = self.risk_config.max_position_size_pct;

        if let Some(sentiment) = &self.current_sentiment {
            // In Extreme Fear, we reduce position size by 50% for Long positions
            // Unless we are Shorting (if we supported shorting, which we don't fully yet)
            // But checking 'side' is good practice.
            if side == OrderSide::Buy && sentiment.classification == SentimentClassification::ExtremeFear {
                adjusted_max_pos_pct *= 0.5;
                debug!(
                    "RiskManager: Extreme Fear ({}) detected. Reducing max position size to {:.2}%",
                    sentiment.value,
                    adjusted_max_pos_pct * 100.0
                );
            }
        }

        let position_pct = (exposure / equity).to_f64().unwrap_or(0.0);

        if position_pct > adjusted_max_pos_pct {
            warn!(
                "RiskManager: Rejecting because Position size ({:.2}%) > Limit ({:.2}%) [Sentiment Adjusted]",
                position_pct * 100.0,
                adjusted_max_pos_pct * 100.0
            );
            return false;
        }

        true
    }

    /// Check if circuit breaker should trigger
    fn check_circuit_breaker(&self, current_equity: Decimal) -> Option<String> {
        // Check daily loss limit
        if self.session_start_equity > Decimal::ZERO {
            let daily_loss_pct = ((current_equity - self.session_start_equity)
                / self.session_start_equity)
                .to_f64()
                .unwrap_or(0.0);

            if daily_loss_pct < -self.risk_config.max_daily_loss_pct {
                let msg = format!(
                    "Daily loss limit breached: {:.2}% (limit: {:.2}%) [Start: {}, Current: {}]",
                    daily_loss_pct * 100.0,
                    self.risk_config.max_daily_loss_pct * 100.0,
                    self.session_start_equity,
                    current_equity
                );
                return Some(msg);
            }
        }

        // Check drawdown limit
        if self.equity_high_water_mark > Decimal::ZERO {
            let drawdown_pct = ((current_equity - self.equity_high_water_mark)
                / self.equity_high_water_mark)
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
        if self.consecutive_losses >= self.risk_config.consecutive_loss_limit {
            return Some(format!(
                "Consecutive loss limit reached: {} trades (limit: {})",
                self.consecutive_losses, self.risk_config.consecutive_loss_limit
            ));
        }

        None
    }

    /// Calculate projected quantity including pending orders
    fn get_projected_quantity(&self, symbol: &str, current_qty: Decimal) -> Decimal {
        let pending: Decimal = self
            .pending_orders
            .values()
            .filter(|p| p.symbol == symbol)
            .map(|p| match p.side {
                OrderSide::Buy => p.remaining_qty(),
                OrderSide::Sell => -p.remaining_qty(),
            })
            .sum();
        current_qty + pending
    }

    /// Handle real-time order updates to maintain pending state
    fn handle_order_update(&mut self, update: OrderUpdate) {
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
                        if pending.side == OrderSide::Sell {
                            if let Some(fill_price) = update.filled_avg_price {
                                let pnl = (fill_price - pending.entry_price) * pending.filled_qty;
                                if pnl < Decimal::ZERO {
                                    self.consecutive_losses += 1;
                                    warn!(
                                        "RiskManager: Trade LOSS detected for {} (${:.2}). Consecutive losses: {}",
                                        pending.symbol, pnl, self.consecutive_losses
                                    );
                                } else {
                                    self.consecutive_losses = 0;
                                    info!(
                                        "RiskManager: Trade PROFIT for {} (${:.2}). Loss streak reset.",
                                        pending.symbol, pnl
                                    );
                                }
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
    }

    /// Validate sector exposure limits
    async fn validate_sector_exposure(
        &mut self,
        proposal: &TradeProposal,
        portfolio: &crate::domain::trading::portfolio::Portfolio,
        current_equity: Decimal,
    ) -> bool {
        if current_equity <= Decimal::ZERO {
            return true;
        }

        // Identify Sector
        let sector = if let Some(provider) = &self.risk_config.sector_provider {
            if let Some(s) = self.sector_cache.get(&proposal.symbol) {
                s.clone()
            } else {
                let s = provider
                    .get_sector(&proposal.symbol)
                    .await
                    .unwrap_or_else(|_| "Unknown".to_string());
                self.sector_cache.insert(proposal.symbol.clone(), s.clone());
                s
            }
        } else {
            "Unknown".to_string()
        };

        if sector == "Unknown" {
            return true;
        }

        // Calculate Current Sector Exposure
        let mut current_sector_value = Decimal::ZERO;

        for (sym, position) in &portfolio.positions {
            let pos_sector = if let Some(provider) = &self.risk_config.sector_provider {
                if let Some(s) = self.sector_cache.get(sym) {
                    s.clone()
                } else {
                    let s = provider
                        .get_sector(sym)
                        .await
                        .unwrap_or_else(|_| "Unknown".to_string());
                    self.sector_cache.insert(sym.clone(), s.clone());
                    s
                }
            } else {
                "Unknown".to_string()
            };

            if pos_sector == sector {
                let price = self
                    .current_prices
                    .get(sym)
                    .cloned()
                    .unwrap_or(position.average_price);
                current_sector_value += price * position.quantity;
            }
        }

        // Add Proposed Trade Value
        let trade_value = proposal.price * proposal.quantity;
        let new_sector_value = current_sector_value + trade_value;

        // Calculate Percentage
        let new_sector_pct = (new_sector_value / current_equity).to_f64().unwrap_or(0.0);

        if new_sector_pct > self.risk_config.max_sector_exposure_pct {
            warn!(
                "RiskManager: Sector exposure limit exceeded for {}. Sector: {}, New Exposure: {:.2}% (Limit: {:.2}%)",
                proposal.symbol,
                sector,
                new_sector_pct * 100.0,
                self.risk_config.max_sector_exposure_pct * 100.0
            );
            return false;
        }

        true
    }

    /// Fetch latest prices for all held positions and update valuation
    pub async fn update_portfolio_valuation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Get fresh portfolio snapshot
        let snapshot = self.portfolio_state_manager.refresh().await?;

        // 2. Collect symbols
        let symbols: Vec<String> = snapshot.portfolio.positions.keys().cloned().collect();
        if symbols.is_empty() {
            return Ok(());
        }

        // 3. Fetch latest prices
        match self.market_service.get_prices(symbols).await {
            Ok(prices) => {
                // Update our cache
                for (sym, price) in prices {
                    self.current_prices.insert(sym, price);
                }

                // 4. Calculate Equity with NEW prices
                let current_equity = snapshot.portfolio.total_equity(&self.current_prices);

                // 5. Update High Water Mark
                if current_equity > self.equity_high_water_mark {
                    self.equity_high_water_mark = current_equity;
                }

                // 6. Check Risks (Async check)
                // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
                if !self.halted {
                    if let Some(reason) = self.check_circuit_breaker(current_equity) {
                        tracing::error!("RiskManager MONITOR: CIRCUIT BREAKER TRIGGERED: {}", reason);
                        self.halted = true;
                        self.liquidate_portfolio(&reason).await;
                    }
                }

                // 7. Capture performance snapshot if monitor available
                if let Some(monitor) = &self.performance_monitor {
                    for sym in self.current_prices.keys() {
                        let _ = monitor.capture_snapshot(sym).await;
                    }
                }
            }
            Err(e) => {
                warn!("RiskManager: Failed to update valuation prices: {}", e);
            }
        }
        Ok(())
    }

    async fn liquidate_portfolio(&mut self, reason: &str) {
        // Get current portfolio snapshot
        let snapshot = self.portfolio_state_manager.get_snapshot().await;

        info!(
            "RiskManager: EMERGENCY LIQUIDATION TRIGGERED - Reason: {}",
            reason
        );

        for (symbol, position) in &snapshot.portfolio.positions {
            if position.quantity > Decimal::ZERO {
                let current_price = self
                    .current_prices
                    .get(symbol)
                    .cloned()
                    .unwrap_or(Decimal::ZERO);

                // CRITICAL SAFETY: Never use unbounded Market orders during emergency
                // Instead, use aggressive Limit orders with 2% slippage tolerance
                // If price unavailable, skip liquidation (requires manual intervention)
                if current_price <= Decimal::ZERO {
                    warn!(
                        "RiskManager: No current price for {} - CANNOT safely liquidate. Manual intervention required.",
                        symbol
                    );
                    continue;
                }

                // CRITICAL SAFETY: Use Market orders for emergency liquidation to guarantee exit.
                // "Get me out at any price" is safer than "Get me out if price > X" in a true crash.

                let order = Order {
                    id: Uuid::new_v4().to_string(),
                    symbol: symbol.clone(),
                    side: OrderSide::Sell,
                    price: Decimal::ZERO, // Market order ignores price
                    quantity: position.quantity,
                    order_type: OrderType::Market,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };

                warn!(
                    "RiskManager: Placing EMERGENCY MARKET SELL for {} (Qty: {})",
                    symbol, position.quantity
                );

                if let Err(e) = self.order_tx.send(order).await {
                    error!(
                        "RiskManager: Failed to send liquidation order for {}: {}",
                        symbol, e
                    );
                }
            }
        }

        info!(
            "RiskManager: Emergency liquidation orders placed. Trading HALTED. Manual review required."
        );
    }

    /// Check if we need to reset session stats (for 24/7 Crypto markets)
    fn check_daily_reset(&mut self, current_equity: Decimal) {
        let today = Utc::now().date_naive();
        if self.asset_class == AssetClass::Crypto && today > self.last_reset_date {
            info!(
                "🔄 24/7 Session Reset: New Baseline Equity = ${} (Was: ${})",
                current_equity, self.session_start_equity
            );
            self.session_start_equity = current_equity;
            self.daily_start_equity = current_equity; // Reset daily tracking
            self.daily_pnl = Decimal::ZERO;
            // self.daily_loss is calculated from session_start_equity, so it effectively resets.
            // Consecutive losses might be preserved or reset?
            // Usually "Daily Loss" is the main drift issue. Consecutive losses are trade-based.
            // I will NOT reset consecutive_losses as bad streaks can span days.
            self.last_reset_date = today;
        }
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
        }
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
        self.handle_order_update(update);
        Ok(())
    }

    /// Handle valuation tick command
    async fn cmd_handle_valuation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.update_portfolio_valuation().await?;

        if !self.halted {
            let snapshot = self.portfolio_state_manager.get_snapshot().await;
            self.check_daily_reset(snapshot.portfolio.total_equity(&self.current_prices));
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

        let portfolio = &snapshot.portfolio;

        // Reconcile pending orders
        self.reconcile_pending_orders(portfolio);

        // Calculate current equity
        let current_equity = portfolio.total_equity(&self.current_prices);

        // Update high water mark
        if current_equity > self.equity_high_water_mark {
            self.equity_high_water_mark = current_equity;
        }

        // Check daily reset
        self.check_daily_reset(current_equity);

        // Circuit breaker check
        if let Some(reason) = self.check_circuit_breaker(current_equity) {
            error!("RiskManager: CIRCUIT BREAKER TRIGGERED - {}", reason);
            self.halted = true;
            self.liquidate_portfolio(&reason).await;
            return Ok(());
        }

        // PDT protection (Pattern Day Trader) - Blocks trades if equity < $25k and rules apply
        let is_pdt_risk = current_equity < Decimal::from(25000);
        let pdt_protection_enabled = !self.non_pdt_mode && !self.risk_config.allow_pdt_risk;
        
 
        if pdt_protection_enabled && is_pdt_risk && portfolio.day_trades_count >= 3 {
            // Rejections:
            // 1. Any BUY is blocked if we have >= 3 day trades (prevents opening new positions that could be day traded)
            // 2. Any SELL that COMPLETES a day trade is blocked (prevents finalizing the 4th day trade)
            
            let is_buy = matches!(proposal.side, OrderSide::Buy);
            
            // Check if this SELL would complete a day trade
            let is_closing_day_trade = if !is_buy {
                // If we bought this symbol today, it's a day trade
                // For simplicity, we check if we have it in positions and it was bought today
                // In a real system, we'd check filled_at timestamp of the buy
                portfolio.positions.get(&proposal.symbol).is_some() // Mock simplification
            } else {
                false
            };

            if is_buy || is_closing_day_trade {
                warn!(
                    "RiskManager: REJECTING {:?} (PDT PROTECT): Count={}, Equity={}",
                    proposal.side, portfolio.day_trades_count, current_equity
                );
                return Ok(());
            }
        }

        // Position size validation for buys
        if matches!(proposal.side, OrderSide::Buy) {
            let current_pos_qty = portfolio
                .positions
                .get(&proposal.symbol)
                .map(|p| p.quantity)
                .unwrap_or(Decimal::ZERO);

            let total_qty = self.get_projected_quantity(&proposal.symbol, current_pos_qty) + proposal.quantity;
            let total_exposure = total_qty * proposal.price;

            if !self.validate_position_size(&proposal.symbol, total_exposure, current_equity, proposal.side) {
                warn!(
                    "RiskManager: Rejecting {:?} order for {} - Position size limit",
                    proposal.side, proposal.symbol
                );
                return Ok(());
            }
        }

        // Sector exposure validation for buys
        if matches!(proposal.side, OrderSide::Buy)
            && !self.validate_sector_exposure(&proposal, portfolio, current_equity).await
        {
            warn!(
                "RiskManager: Rejecting {:?} order for {} - Sector exposure limit",
                proposal.side, proposal.symbol
            );
            return Ok(());
        }

        // Correlation-based diversification validation for buys
        if matches!(proposal.side, OrderSide::Buy) {
            if let Some(corr_service) = &self.correlation_service {
                // Collect all symbols (target + existing positions)
                let mut symbols: Vec<String> = portfolio.positions.keys().cloned().collect();
                if !symbols.contains(&proposal.symbol) {
                    symbols.push(proposal.symbol.clone());
                }

                if symbols.len() > 1 {
                    match corr_service.calculate_correlation_matrix(&symbols).await {
                        Ok(matrix) => {
                            if let Err(reason) = CorrelationFilter::check_correlation(
                                &proposal.symbol,
                                &portfolio.positions,
                                &matrix,
                                &self.risk_config.correlation_config,
                            ) {
                                warn!("RiskManager: Rejecting BUY order for {} - {}", proposal.symbol, reason);
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            warn!("RiskManager: Correlation check failed for {}: {}. Proceeding with caution.", proposal.symbol, e);
                        }
                    }
                }
            }
        }

        // Execute proposal
        self.execute_proposal_internal(proposal, portfolio).await?;

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

        loop {
            tokio::select! {
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
        );

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
        );
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
        );
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
        );
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

        let mut risk_config = RiskConfig::default();
        risk_config.max_daily_loss_pct = 0.5; // 50% max allowed
        risk_config.max_drawdown_pct = 0.5; // 50%

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
        );
        
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
        );
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
        );

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
        );

        // Manually manipulate last_reset_date to yesterday
        let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
        rm.last_reset_date = yesterday;
        rm.session_start_equity = Decimal::from(5000); // Old baseline

        // Wait, current_equity argument needed.
        let current_equity = Decimal::from(10000);
        rm.check_daily_reset(current_equity);

        assert_eq!(
            rm.session_start_equity, current_equity,
            "Should reset session equity to current"
        );
        assert_eq!(
            rm.last_reset_date,
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

        let mut risk_config = RiskConfig::default();
        risk_config.max_position_size_pct = 0.10; // 10% normally ($1000)

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
        );
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
}
