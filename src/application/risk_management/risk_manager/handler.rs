use crate::application::risk_management::circuit_breaker_service::HaltLevel;
use crate::application::risk_management::commands::RiskCommand;
use crate::application::risk_management::order_reconciler::PendingOrder;
use crate::application::risk_management::risk_manager::RiskManager;
use crate::domain::ports::OrderUpdate;
use crate::domain::risk::filters::{ValidationContext, ValidationResult};
use crate::domain::risk::risk_config::RiskConfig;
use crate::domain::sentiment::Sentiment;
use crate::domain::trading::types::{Order, OrderSide, TradeProposal};
use chrono::Utc;
use rust_decimal::Decimal;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

impl RiskManager {
    // ============================================================================
    // COMMAND PATTERN HANDLERS
    // ============================================================================

    /// Handle internal commands
    pub async fn handle_command(&mut self, command: RiskCommand) -> anyhow::Result<()> {
        match command {
            RiskCommand::OrderUpdate(update) => self.cmd_handle_order_update(update).await,
            RiskCommand::ValuationTick => self.cmd_handle_valuation().await,
            RiskCommand::RefreshPortfolio => self.cmd_handle_refresh().await,
            RiskCommand::ProcessProposal(proposal) => self.cmd_handle_proposal(proposal).await,
            RiskCommand::UpdateSentiment(sentiment) => {
                self.cmd_handle_update_sentiment(sentiment).await
            }
            RiskCommand::UpdateConfig(config) => self.cmd_handle_update_config(config).await,
            RiskCommand::CircuitBreakerTrigger => {
                warn!(
                    "RiskManager: MANUAL CIRCUIT BREAKER TRIGGERED! Executing Panic Liquidation."
                );
                self.circuit_breaker_service.set_halted(HaltLevel::FullHalt);
                self.metrics.circuit_breaker_status.set(1.0);
                self.liquidate_portfolio("Manual Circuit Breaker Trigger")
                    .await;
                Ok(())
            }
        }
    }

    async fn cmd_handle_update_config(&mut self, config: Box<RiskConfig>) -> anyhow::Result<()> {
        info!("RiskManager: Updating risk configuration: {:?}", config);
        self.risk_config = *config;
        Ok(())
    }

    async fn cmd_handle_update_sentiment(&mut self, sentiment: Sentiment) -> anyhow::Result<()> {
        let sym = sentiment.symbol.as_deref().unwrap_or("UNKNOWN");
        info!(
            "RiskManager: Received Market Sentiment for {}: {} ({})",
            sym, sentiment.value, sentiment.classification
        );
        if let Some(symbol) = sentiment.symbol.clone() {
            self.symbol_sentiments.insert(symbol, sentiment);
        }
        Ok(())
    }

    /// Handle portfolio refresh command
    async fn cmd_handle_refresh(&mut self) -> anyhow::Result<()> {
        self.portfolio_state_manager.refresh().await?;
        Ok(())
    }

    /// Handle order update command
    #[instrument(skip(self, update), fields(symbol = %update.symbol, order_id = %update.order_id, status = ?update.status, correlation_id = tracing::field::Empty))]
    async fn cmd_handle_order_update(&mut self, update: OrderUpdate) -> anyhow::Result<()> {
        let correlation_id = self
            .order_reconciler
            .pending_orders
            .get(&update.client_order_id)
            .and_then(|p| p.correlation_id.clone());
        if let Some(ref cid) = correlation_id {
            tracing::Span::current().record("correlation_id", cid);
        }

        if self.handle_order_update(update).await {
            self.persist_state().await;
        }
        Ok(())
    }

    /// Handle valuation tick command
    async fn cmd_handle_valuation(&mut self) -> anyhow::Result<()> {
        self.update_portfolio_valuation().await?;
        if !self.circuit_breaker_service.is_halted() {
            let snapshot = self.portfolio_state_manager.get_snapshot().await;
            if self.check_daily_reset(snapshot.portfolio.total_equity(&self.current_prices)) {
                self.persist_state().await;
            }
        }

        // Always reconcile pending orders regardless of circuit breaker state.
        // Stale reservations must be released to avoid permanently locking capital.
        let snapshot = self.portfolio_state_manager.get_snapshot().await;
        self.reconcile_pending_orders(&snapshot.portfolio).await;

        Ok(())
    }

    /// Handle trade proposal command
    #[instrument(skip(self, proposal), fields(symbol = %proposal.symbol, side = ?proposal.side, correlation_id = ?proposal.correlation_id))]
    async fn cmd_handle_proposal(&mut self, proposal: TradeProposal) -> anyhow::Result<()> {
        let level = self.circuit_breaker_service.halt_level();
        if level == HaltLevel::Reduced || level == HaltLevel::FullHalt {
            info!(
                "RiskManager: Trading HALTED ({:?}). Rejecting proposal for {}",
                level, proposal.symbol
            );
            return Ok(());
        }
        let mut proposal = proposal;
        if level == HaltLevel::Warning {
            let mult = rust_decimal::Decimal::from_f64_retain(HaltLevel::Warning.size_multiplier())
                .unwrap_or(Decimal::ONE);
            proposal.quantity = (proposal.quantity * mult).round_dp(4);
            if proposal.quantity <= Decimal::ZERO {
                info!(
                    "RiskManager: Proposal for {} scaled to zero under Warning level, skipping",
                    proposal.symbol
                );
                return Ok(());
            }
            debug!(
                "RiskManager: Circuit breaker Warning: reduced proposal size for {} by 50%",
                proposal.symbol
            );
        }

        // --- STALE DATA GUARD ---
        // Use ConnectionHealthService as the single source of truth for market data freshness.
        // It properly tracks the last received data event independently of proposal processing.
        if self
            .connection_health_service
            .get_market_data_status()
            .await
            == crate::application::monitoring::connection_health_service::ConnectionStatus::Offline
        {
            info!(
                "RiskManager: Market Data OFFLINE. Rejecting proposal for {}",
                proposal.symbol
            );
            return Ok(());
        }
        // -------------------------

        info!("RiskManager: reviewing proposal {:?}", proposal);

        // Update current price
        let now = Utc::now().timestamp();
        self.current_prices
            .insert(proposal.symbol.clone(), proposal.price);
        self.last_quote_timestamp = now; // Track for metrics/debugging

        // Get portfolio snapshot
        let mut snapshot = self.portfolio_state_manager.get_snapshot().await;

        // Refresh if stale
        if self.portfolio_state_manager.is_stale(&snapshot) {
            snapshot = self.portfolio_state_manager.refresh().await?;
        }

        // Reconcile pending orders
        self.reconcile_pending_orders(&snapshot.portfolio).await;

        // Calculate current equity
        let current_equity = snapshot.portfolio.total_equity(&self.current_prices);

        // Update high water mark
        if current_equity > self.state_manager.get_state().equity_high_water_mark {
            self.state_manager.get_state_mut().equity_high_water_mark = current_equity;
        }

        // Check daily reset
        if self.check_daily_reset(current_equity) {
            self.persist_state().await;
        }

        // Circuit breaker check (Trigger Liquidation logic)
        if let Some((level, reason)) = self.check_circuit_breaker(current_equity) {
            let current_level = self.circuit_breaker_service.halt_level();
            if level > current_level {
                self.trigger_alert(level, &reason);
                self.circuit_breaker_service.set_halted(level);
                self.metrics.circuit_breaker_status.set(1.0);

                // Grace Period: skip emergency liquidation during first 60 seconds
                if Utc::now().timestamp() - self.startup_time < 60 {
                    warn!(
                        "RiskManager: CIRCUIT BREAKER TRIGGERED ({:?}) during startup grace period. skipping liquidation for stabilization.",
                        level
                    );
                } else {
                    self.liquidate_portfolio(&reason).await;
                }
            }
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
            service.get_correlation_matrix(&symbols).await.ok()
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

        let pending_exposure = self
            .order_reconciler
            .get_pending_exposure(&proposal.symbol, OrderSide::Buy);

        let recent_candles = if let Some(repo) = &self.candle_repository {
            // Fetch last 20 recent candles for price anomaly validation
            // We use a safe lookback window (e.g. 5 min * 20 = 100 min)
            let now_ts = Utc::now().timestamp();
            // Assumed get_range handles sort order
            repo.get_range(&proposal.symbol, now_ts - 7200, now_ts)
                .await
                .ok()
        } else {
            None
        };
        // NOTE: We pass a reference to the vector if it exists.
        // Since `recent_candles` is owned here, we need to be careful with lifetimes.
        // ValidationContext expects `Option<&'a [Candle]>`.

        let candles_ref = recent_candles.as_deref();

        let available_cash = snapshot.available_cash();

        let symbol_sentiment = self.symbol_sentiments.get(&proposal.symbol);

        let ctx = ValidationContext::new(
            &proposal,
            &snapshot.portfolio,
            current_equity,
            &self.current_prices,
            self.state_manager.get_state(),
            symbol_sentiment,
            correlation_matrix.as_ref(), // Pass pre-calculated matrix
            volatility_multiplier,
            pending_exposure,
            available_cash,
            candles_ref, // Pass recent candles from CandleRepository for PriceAnomalyValidator
        );

        // Execute Pipeline
        match self.validation_pipeline.validate(&ctx).await {
            ValidationResult::Approve => {
                // Reserve exposure for BUY orders to prevent over-allocation.
                // This ensures that subsequent proposals see reduced available_cash
                // and won't exceed the actual balance at the broker.
                let reservation_token = if proposal.side == OrderSide::Buy {
                    let order_cost = proposal.price * proposal.quantity;
                    match self
                        .portfolio_state_manager
                        .reserve_exposure(&proposal.symbol, order_cost, snapshot.version)
                        .await
                    {
                        Ok(token) => {
                            info!(
                                "RiskManager: Reserved ${} for {} (token: {})",
                                order_cost,
                                proposal.symbol,
                                &token.id[..8]
                            );
                            Some(token)
                        }
                        Err(e) => {
                            // Version conflict or insufficient funds after reservation accounting.
                            // Retry once with a fresh snapshot.
                            match self.portfolio_state_manager.refresh().await {
                                Ok(fresh) => {
                                    match self
                                        .portfolio_state_manager
                                        .reserve_exposure(
                                            &proposal.symbol,
                                            order_cost,
                                            fresh.version,
                                        )
                                        .await
                                    {
                                        Ok(token) => Some(token),
                                        Err(retry_err) => {
                                            info!(
                                                "RiskManager: Reservation failed for {} after retry: {}. \
                                                 Rejecting to prevent over-allocation.",
                                                proposal.symbol, retry_err
                                            );
                                            return Ok(());
                                        }
                                    }
                                }
                                Err(refresh_err) => {
                                    info!(
                                        "RiskManager: Portfolio refresh failed during reservation for {}: {}. \
                                         Original error: {}. Rejecting proposal.",
                                        proposal.symbol, refresh_err, e
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    }
                } else {
                    None
                };

                // All checks passed — submit order with reservation
                self.execute_proposal_internal(proposal, reservation_token)
                    .await?;
            }
            ValidationResult::Reject(reason) => {
                info!(
                    "RiskManager: Rejecting {:?} order for {} - {}",
                    proposal.side, proposal.symbol, reason
                );
            }
        }

        Ok(())
    }

    /// Internal proposal execution logic (extracted from run())
    ///
    /// Accepts an optional `ReservationToken` for BUY orders that tracks the
    /// reserved capital in `PortfolioStateManager`. The reservation is released
    /// automatically when the order completes (fill/reject/cancel) via
    /// `OrderReconciler::remove_order`.
    #[instrument(skip(self, proposal, reservation_token), fields(symbol = %proposal.symbol, side = ?proposal.side, correlation_id = ?proposal.correlation_id))]
    async fn execute_proposal_internal(
        &mut self,
        proposal: TradeProposal,
        reservation_token: Option<
            crate::application::monitoring::portfolio_state_manager::ReservationToken,
        >,
    ) -> anyhow::Result<()> {
        // Create order with correct structure
        let order = Order {
            id: Uuid::new_v4().to_string(),
            symbol: proposal.symbol.clone(),
            side: proposal.side,
            price: proposal.price,
            quantity: proposal.quantity,
            order_type: proposal.order_type,
            status: crate::domain::trading::types::OrderStatus::Pending,
            timestamp: Utc::now().timestamp_millis(),
            correlation_id: proposal.correlation_id.clone(),
            stop_loss: proposal.stop_loss,
        };

        // Track as pending
        self.order_reconciler.track_order(
            order.id.clone(),
            PendingOrder {
                symbol: proposal.symbol.clone(),
                side: proposal.side,
                requested_qty: proposal.quantity,
                filled_qty: Decimal::ZERO,
                filled_but_not_synced: false,
                entry_price: proposal.price,
                filled_at: None,
                submitted_at: Utc::now().timestamp_millis(),
                correlation_id: proposal.correlation_id.clone(),
            },
        );

        // Associate reservation token with order so it is released on completion
        if let Some(token) = reservation_token {
            self.order_reconciler
                .add_reservation(order.id.clone(), token);
        }

        // Submit order
        info!(
            symbol = %proposal.symbol,
            side = ?proposal.side,
            qty = %proposal.quantity,
            price = %proposal.price,
            correlation_id = ?proposal.correlation_id,
            "RiskManager: Submitting order"
        );

        if let Err(e) = self.order_tx.send(order.clone()).await {
            error!(error = %e, "RiskManager: Failed to send order");
            if let Some(token) = self.order_reconciler.remove_order(&order.id) {
                self.portfolio_state_manager
                    .release_reservation(token)
                    .await;
            }
            return Err(anyhow::anyhow!("Failed to send order: {}", e));
        }

        Ok(())
    }
}
