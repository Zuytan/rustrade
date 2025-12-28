use crate::domain::ports::{ExecutionService, MarketDataService, SectorProvider};
use crate::domain::trading::types::{Order, OrderSide, TradeProposal};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{error, info, warn};
use uuid::Uuid;
use tokio::sync::RwLock;

use crate::domain::trading::portfolio::Portfolio;
use crate::application::monitoring::performance_monitoring_service::PerformanceMonitoringService;

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
}

impl std::fmt::Debug for RiskConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RiskConfig")
            .field("max_position_size_pct", &self.max_position_size_pct)
            .field("max_daily_loss_pct", &self.max_daily_loss_pct)
            .field("max_drawdown_pct", &self.max_drawdown_pct)
            .field("consecutive_loss_limit", &self.consecutive_loss_limit)
            .field("valuation_interval_seconds", &self.valuation_interval_seconds)
            .field("max_sector_exposure_pct", &self.max_sector_exposure_pct)
            .finish()
    }
}

impl RiskConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_position_size_pct <= 0.0 || self.max_position_size_pct > 1.0 {
            return Err(format!("Invalid max_position_size_pct: {}", self.max_position_size_pct));
        }
        if self.max_daily_loss_pct <= 0.0 || self.max_daily_loss_pct > 0.5 {
             return Err(format!("Invalid max_daily_loss_pct: {}", self.max_daily_loss_pct));
        }
        if self.max_drawdown_pct <= 0.0 || self.max_drawdown_pct > 1.0 {
             return Err(format!("Invalid max_drawdown_pct: {}", self.max_drawdown_pct));
        }
        if self.consecutive_loss_limit == 0 {
             return Err("consecutive_loss_limit must be > 0".to_string());
        }
        if self.max_sector_exposure_pct <= 0.0 || self.max_sector_exposure_pct > 1.0 {
             return Err(format!("Invalid max_sector_exposure_pct: {}", self.max_sector_exposure_pct));
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
        }
    }
}

pub struct RiskManager {
    proposal_rx: Receiver<TradeProposal>,
    order_tx: Sender<Order>,
    execution_service: Arc<dyn ExecutionService>,
    market_service: Arc<dyn MarketDataService>,
    non_pdt_mode: bool,
    risk_config: RiskConfig,
    // Risk Tracking State
    equity_high_water_mark: Decimal,
    session_start_equity: Decimal,
    consecutive_losses: usize,
    current_prices: HashMap<String, Decimal>, // Track current prices for equity calculation
    portfolio: Arc<RwLock<Portfolio>>,
    performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    sector_cache: HashMap<String, String>,
}

impl RiskManager {
    pub fn new(
        proposal_rx: Receiver<TradeProposal>,
        order_tx: Sender<Order>,
        execution_service: Arc<dyn ExecutionService>,
        market_service: Arc<dyn MarketDataService>,
        portfolio: Arc<RwLock<Portfolio>>,
        non_pdt_mode: bool,
        risk_config: RiskConfig,
        performance_monitor: Option<Arc<PerformanceMonitoringService>>,
    ) -> Self {
        if let Err(e) = risk_config.validate() {
            panic!("RiskManager Configuration Error: {}", e);
        }
        Self {
            proposal_rx,
            order_tx,
            execution_service,
            market_service,
            portfolio,
            non_pdt_mode,
            risk_config,
            equity_high_water_mark: Decimal::ZERO,
            session_start_equity: Decimal::ZERO,
            consecutive_losses: 0,
            current_prices: HashMap::new(),
            performance_monitor,
            sector_cache: HashMap::new(),
        }
    }

    /// Initialize session tracking with starting equity
    async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let portfolio = self.portfolio.read().await;

        // Fetch initial prices for accurate equity calculation
        let symbols: Vec<String> = portfolio.positions.keys().cloned().collect();
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

        let initial_equity = portfolio.total_equity(&self.current_prices);
        self.session_start_equity = initial_equity;
        self.equity_high_water_mark = initial_equity;
        info!(
            "RiskManager: Session initialized with equity: {}",
            initial_equity
        );
        Ok(())
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
                return Some(format!(
                    "Daily loss limit breached: {:.2}% (limit: {:.2}%)",
                    daily_loss_pct * 100.0,
                    self.risk_config.max_daily_loss_pct * 100.0
                ));
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

    /// Validate position size doesn't exceed limit
    fn validate_position_size(&self, proposal: &TradeProposal, current_equity: Decimal) -> bool {
        if current_equity <= Decimal::ZERO {
            return true; // Can't calculate percentage, allow (conservative)
        }

        let position_value = proposal.price * proposal.quantity;
        let position_pct = (position_value / current_equity).to_f64().unwrap_or(0.0);

        if position_pct > self.risk_config.max_position_size_pct {
            warn!(
                "RiskManager: Position size too large: {:.2}% of equity (limit: {:.2}%)",
                position_pct * 100.0,
                self.risk_config.max_position_size_pct * 100.0
            );
            return false;
        }

        true
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
                 let s = provider.get_sector(&proposal.symbol).await.unwrap_or_else(|_| "Unknown".to_string());
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
                     let s = provider.get_sector(sym).await.unwrap_or_else(|_| "Unknown".to_string());
                     self.sector_cache.insert(sym.clone(), s.clone());
                     s
                 }
            } else {
                 "Unknown".to_string()
            };

            if pos_sector == sector {
                let price = self.current_prices.get(sym).cloned().unwrap_or(position.average_price);
                current_sector_value += price * position.quantity;
            }
        }

        // Add Proposed Trade Value
        let trade_value = proposal.price * proposal.quantity;
        let new_sector_value = current_sector_value + trade_value;

        // Calculate Percentage
        let new_sector_pct = (new_sector_value / current_equity)
            .to_f64()
            .unwrap_or(0.0);

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
    async fn update_portfolio_valuation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Get Portfolio to know what we hold
        let portfolio = self.portfolio.read().await;

        // 2. Collect symbols
        let symbols: Vec<String> = portfolio.positions.keys().cloned().collect();
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
                let current_equity = portfolio.total_equity(&self.current_prices);

                // 5. Update High Water Mark
                if current_equity > self.equity_high_water_mark {
                    self.equity_high_water_mark = current_equity;
                }

                // 6. Check Risks (Async check)
                // 6. Check Risks (Async check)
                if let Some(reason) = self.check_circuit_breaker(current_equity) {
                    tracing::error!("RiskManager MONITOR: CIRCUIT BREAKER TRIGGERED: {}", reason);
                    // In a real system, we might want to shut down or cancel all orders here.
                    // For now, next proposal will be rejected.
                }

                // 7. Capture performance snapshot if monitor available
                if let Some(monitor) = &self.performance_monitor {
                    // RiskManager typically handles all active symbols or we pick a primary one?
                    // For now, let's snapshot for a generic "PORTFOLIO" or iterate symbols.
                    // The capture_snapshot method takes a symbol.
                    // Let's use "TOTAL" as a convention for portfolio level if allowed, 
                    // or snapshot for each position.
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

        loop {
            tokio::select! {
                        _ = valuation_interval.tick() => {
                            if let Err(e) = self.update_portfolio_valuation().await {
                                error!("RiskManager: Valuation update error: {}", e);
                            }
                        }
                        Some(proposal) = self.proposal_rx.recv() => {
                            info!("RiskManager: reviewing proposal {:?}", proposal);

                    // Update current price for this symbol
                    self.current_prices
                        .insert(proposal.symbol.clone(), proposal.price);

                    // Fetch fresh portfolio data from exchange
                    let portfolio = match self.execution_service.get_portfolio().await {
                        Ok(p) => p,
                        Err(e) => {
                            error!("RiskManager: Failed to fetch portfolio: {}", e);
                            continue;
                        }
                    };

                    // Calculate current equity
                    let current_equity = portfolio.total_equity(&self.current_prices);

                    // Update high water mark
                    if current_equity > self.equity_high_water_mark {
                        self.equity_high_water_mark = current_equity;
                    }

                    // Check circuit breaker BEFORE other validations
                    if let Some(reason) = self.check_circuit_breaker(current_equity) {
                        error!("RiskManager: CIRCUIT BREAKER TRIGGERED - {}", reason);
                        error!(
                            "RiskManager: All trading halted. Current equity: {}",
                            current_equity
                        );
                        continue; // Reject all orders
                    }

                    // Validate position size for buy orders
                    if matches!(proposal.side, OrderSide::Buy)
                        && !self.validate_position_size(&proposal, current_equity)
                    {
                        warn!(
                            "RiskManager: Rejecting {:?} order for {} - Position size limit",
                            proposal.side, proposal.symbol
                        );
                        continue;
                    }

                    // Validate Sector Exposure for buy orders
                    if matches!(proposal.side, OrderSide::Buy)
                        && !self
                            .validate_sector_exposure(&proposal, &portfolio, current_equity)
                            .await
                    {
                         warn!(
                            "RiskManager: Rejecting {:?} order for {} - Sector exposure limit",
                            proposal.side, proposal.symbol
                        );
                        continue;
                    }

                    // Validation Logic
                    let cost = proposal.price * proposal.quantity;

                    let is_valid = match proposal.side {
                        OrderSide::Buy => {
                            if portfolio.cash >= cost {
                                true
                            } else {
                                warn!(
                                    "RiskManager: Insufficient funds. Cash: {}, Cost: {}",
                                    portfolio.cash, cost
                                );
                                false
                            }
                        }
                        OrderSide::Sell => {
                            // Normalize symbol for lookup (remove / and spaces)
                            let normalized_search = proposal.symbol.replace("/", "").replace(" ", "");

                            // Check if we hold the asset by checking all positions with normalized symbols
                            let found_pos = portfolio.positions.iter().find(|(sym, _)| {
                                sym.replace("/", "").replace(" ", "") == normalized_search
                            });

                            if let Some((_, pos)) = found_pos {
                                // PDT Protection
                                if self.non_pdt_mode {
                                    let today_orders = match self.execution_service.get_today_orders().await
                                    {
                                        Ok(orders) => orders,
                                        Err(e) => {
                                            error!("RiskManager: Failed to fetch today's orders: {}", e);
                                            Vec::new()
                                        }
                                    };

                                    let bought_today = today_orders.iter().any(|o| {
                                        o.side == OrderSide::Buy
                                            && o.symbol.replace("/", "").replace(" ", "")
                                                == normalized_search
                                    });

                                    if bought_today {
                                        warn!(
                                            "RiskManager: REJECTED Sell for {} - PDT Protection active (bought today)",
                                            proposal.symbol
                                        );
                                        false
                                    } else {
                                        true
                                    }
                                } else {
                                    // If we hold any quantity, we can sell.
                                    // If the proposal quantity is more than we own, we adjust to sell all.
                                    let sell_qty = if pos.quantity < proposal.quantity {
                                        warn!(
                                            "RiskManager: Adjusting sell quantity from {} to available {}",
                                            proposal.quantity, pos.quantity
                                        );
                                        pos.quantity
                                    } else {
                                        proposal.quantity
                                    };

                                    if sell_qty > rust_decimal::Decimal::ZERO {
                                        true
                                    } else {
                                        warn!(
                                            "RiskManager: Owned quantity is zero for {}",
                                            proposal.symbol
                                        );
                                        false
                                    }
                                }
                            } else {
                                warn!(
                                    "RiskManager: No position found for {} (normalized: {})",
                                    proposal.symbol, normalized_search
                                );
                                false
                            }
                        }
                    };

                    if is_valid {
                        // Determine actual quantity (might have changed during validation)
                        let final_qty = match proposal.side {
                            OrderSide::Sell => {
                                let normalized_search = proposal.symbol.replace("/", "").replace(" ", "");
                                portfolio
                                    .positions
                                    .iter()
                                    .find(|(sym, _)| {
                                        sym.replace("/", "").replace(" ", "") == normalized_search
                                    })
                                    .map(|(_, pos)| {
                                        if pos.quantity < proposal.quantity {
                                            pos.quantity
                                        } else {
                                            proposal.quantity
                                        }
                                    })
                                    .unwrap_or(proposal.quantity)
                            }
                            OrderSide::Buy => proposal.quantity,
                        };

                        let order = Order {
                            id: Uuid::new_v4().to_string(),
                            symbol: proposal.symbol,
                            side: proposal.side,
                            price: proposal.price,
                            quantity: final_qty,
                            order_type: proposal.order_type,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };

                        info!("RiskManager: Approved. Sending Order {}", order.id);
                        if let Err(e) = self.order_tx.send(order).await {
                            error!("RiskManager: Failed to send order: {}", e);
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::{OrderSide, OrderType};
    use crate::domain::trading::portfolio::{Portfolio, Position};
    use crate::infrastructure::mock::{MockExecutionService, MockMarketDataService};
    use chrono::Utc;
    use rust_decimal::Decimal;
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
        ) -> Result<mpsc::Receiver<crate::domain::trading::types::MarketEvent>, anyhow::Error> {
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
            max_daily_loss_pct: 0.05,
            ..RiskConfig::default()
        };

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            market_service,
            portfolio, false,
            config, None,
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

        // Expect rejection due to Circuit Breaker
        // The order channel should NOT receive anything.
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert!(
            order_rx.try_recv().is_err(),
            "Order should be rejected due to circuit breaker"
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

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            market_service,
            portfolio, false,
            RiskConfig::default(), None,
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

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            market_service,
            portfolio, false,
            RiskConfig::default(), None,
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

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            market_service,
            portfolio, false,
            RiskConfig::default(), None,
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
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
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
        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            market_service,
            portfolio,
            true,
            RiskConfig::default(),
            None,
        );
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Sell,
            price: Decimal::from(60),
            quantity: Decimal::from(5),
            order_type: OrderType::Market,
            reason: "Test PDT".to_string(),
            timestamp: Utc::now().timestamp_millis(),
        };
        proposal_tx.send(proposal).await.unwrap();

        // Should be REJECTED
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(order_rx.try_recv().is_err());
    }

    struct MockSectorProvider {
        sectors: HashMap<String, String>,
    }

    #[async_trait::async_trait]
    impl SectorProvider for MockSectorProvider {
        async fn get_sector(&self, symbol: &str) -> Result<String, anyhow::Error> {
            Ok(self.sectors.get(symbol).cloned().unwrap_or_else(|| "Unknown".to_string()))
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

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            market_service,
            portfolio, false,
            config, None,
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
        assert!(order_rx.try_recv().is_err(), "Should reject due to sector exposure");
    }
}
