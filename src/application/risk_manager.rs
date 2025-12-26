use crate::domain::ports::ExecutionService;
use crate::domain::types::{Order, OrderSide, TradeProposal};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{error, info, warn};
use uuid::Uuid;

/// Risk management configuration
#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub max_position_size_pct: f64, // Max % of equity per position (e.g., 0.25 = 25%)
    pub max_daily_loss_pct: f64,    // Max % loss per day (e.g., 0.02 = 2%)
    pub max_drawdown_pct: f64,      // Max % drawdown from high water mark (e.g., 0.10 = 10%)
    pub consecutive_loss_limit: usize, // Max consecutive losing trades before halt
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_size_pct: 0.25, // 25% max
            max_daily_loss_pct: 0.02,    // 2% daily loss limit
            max_drawdown_pct: 0.10,      // 10% max drawdown
            consecutive_loss_limit: 3,   // 3 consecutive losses
        }
    }
}

pub struct RiskManager {
    proposal_rx: Receiver<TradeProposal>,
    order_tx: Sender<Order>,
    execution_service: Arc<dyn ExecutionService>,
    non_pdt_mode: bool,
    risk_config: RiskConfig,
    // Risk Tracking State
    equity_high_water_mark: Decimal,
    session_start_equity: Decimal,
    consecutive_losses: usize,
    current_prices: HashMap<String, Decimal>, // Track current prices for equity calculation
}

impl RiskManager {
    pub fn new(
        proposal_rx: Receiver<TradeProposal>,
        order_tx: Sender<Order>,
        execution_service: Arc<dyn ExecutionService>,
        non_pdt_mode: bool,
        risk_config: RiskConfig,
    ) -> Self {
        Self {
            proposal_rx,
            order_tx,
            execution_service,
            non_pdt_mode,
            risk_config,
            equity_high_water_mark: Decimal::ZERO,
            session_start_equity: Decimal::ZERO,
            consecutive_losses: 0,
            current_prices: HashMap::new(),
        }
    }

    /// Initialize session tracking with starting equity
    async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let portfolio = self.execution_service.get_portfolio().await?;
        let initial_equity = portfolio.cash;
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

    pub async fn run(&mut self) {
        info!("RiskManager started with config: {:?}", self.risk_config);

        // Initialize session
        if let Err(e) = self.initialize_session().await {
            error!("RiskManager: Failed to initialize session: {}", e);
        }

        while let Some(proposal) = self.proposal_rx.recv().await {
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
            if matches!(proposal.side, OrderSide::Buy) {
                if !self.validate_position_size(&proposal, current_equity) {
                    warn!(
                        "RiskManager: Rejecting {:?} order for {} - Position size limit",
                        proposal.side, proposal.symbol
                    );
                    continue;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::portfolio::{Portfolio, Position};
    use crate::infrastructure::mock::MockExecutionService;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use tokio::sync::{RwLock, mpsc};

    #[tokio::test]
    async fn test_buy_approval() {
        let (proposal_tx, proposal_rx) = mpsc::channel(1);
        let (order_tx, mut order_rx) = mpsc::channel(1);
        let mut port = Portfolio::new();
        port.cash = Decimal::from(1000);
        let portfolio = Arc::new(RwLock::new(port));
        let exec_service = Arc::new(MockExecutionService::new(portfolio));

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            false,
            RiskConfig::default(),
        );
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(1),
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
        let exec_service = Arc::new(MockExecutionService::new(portfolio));

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            false,
            RiskConfig::default(),
        );
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Buy,
            price: Decimal::from(100),
            quantity: Decimal::from(1),
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
        let exec_service = Arc::new(MockExecutionService::new(portfolio));

        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            false,
            RiskConfig::default(),
        );
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Sell,
            price: Decimal::from(100),
            quantity: Decimal::from(5), // Sell 5
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
        let exec_service = Arc::new(MockExecutionService::new(portfolio));

        // Simulate a BUY today
        exec_service
            .execute(Order {
                id: "buy1".to_string(),
                symbol: "ABC".to_string(),
                side: OrderSide::Buy,
                price: Decimal::from(50),
                quantity: Decimal::from(10),
                timestamp: Utc::now().timestamp_millis(),
            })
            .await
            .unwrap();

        // New RiskManager with NON_PDT_MODE = true
        let mut rm = RiskManager::new(
            proposal_rx,
            order_tx,
            exec_service,
            true,
            RiskConfig::default(),
        );
        tokio::spawn(async move { rm.run().await });

        let proposal = TradeProposal {
            symbol: "ABC".to_string(),
            side: OrderSide::Sell,
            price: Decimal::from(60),
            quantity: Decimal::from(5),
            reason: "Test PDT".to_string(),
            timestamp: Utc::now().timestamp_millis(),
        };
        proposal_tx.send(proposal).await.unwrap();

        // Should be REJECTED
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(order_rx.try_recv().is_err());
    }
}
