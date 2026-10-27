//! Portfolio valuation, volatility tracking, circuit breaker checks, order
//! reconciliation and emergency liquidation.

use super::*;

impl RiskManager {
    /// Check if circuit breaker should trigger; returns level and message when triggered.
    pub(super) fn check_circuit_breaker(
        &self,
        current_equity: Decimal,
    ) -> Option<(HaltLevel, String)> {
        self.circuit_breaker_service
            .check_circuit_breaker(self.state_manager.get_state(), current_equity)
    }

    /// Handle real-time order updates to maintain pending state
    /// Returns true if risk state (e.g. consecutive losses) changed and needs persistence.
    pub(super) async fn handle_order_update(&mut self, update: OrderUpdate) -> bool {
        let (state_changed, token) = self
            .order_reconciler
            .handle_order_update(&update, self.state_manager.get_state_mut());

        // Release reservation token synchronously in this async context
        if let Some(t) = token {
            self.portfolio_state_manager.release_reservation(t).await;
        }

        state_changed
    }

    /// Fetch latest prices for all held positions and update valuation
    /// Delegates to PortfolioValuationService for valuation updates
    pub async fn update_portfolio_valuation(&mut self) -> anyhow::Result<()> {
        // Delegate valuation to PortfolioValuationService
        let (portfolio, current_equity) = self
            .portfolio_valuation_service
            .update_portfolio_valuation(&mut self.current_prices)
            .await?;

        // Update volatility
        let _ = self.portfolio_valuation_service.update_volatility().await;

        // Update High Water Mark via State Manager
        self.state_manager.update(current_equity, Utc::now()).await;

        // Check Risks (Async check)
        // Only trigger circuit breaker if not already halted (prevents duplicate liquidations)
        if !self.circuit_breaker_service.is_halted() {
            // SAFEGUARD: Ensure we have prices for ALL positions before running circuit breaker.
            // If we fall back to average_price in total_equity, we might trigger a false drawdown
            // if the asset has appreciated significantly since entry.
            // (e.g. HWM based on $150, but we use AvgPrice $100 -> -33% Drawdown -> PANIC)
            // Note: portfolio is not returned by update_portfolio_valuation but we can get it from state manager
            // actually update_portfolio_valuation returns (Portfolio, Decimal) based on my reading of lines 336-339
            // Wait, looking at lines 336-339 in the original file view:
            // let (_portfolio, current_equity) = self...
            // So I need to capture the portfolio variable.

            let has_missing_prices = portfolio
                .positions
                .keys()
                .any(|symbol| !self.current_prices.contains_key(symbol));

            if has_missing_prices {
                // Log warning but DO NOT trigger circuit breaker
                let missing: Vec<_> = portfolio
                    .positions
                    .keys()
                    .filter(|s| !self.current_prices.contains_key(*s))
                    .collect();
                tracing::warn!(
                    "RiskManager: Skipping Circuit Breaker check due to missing prices for: {:?}",
                    missing
                );
            } else if let Some((level, reason)) = self.check_circuit_breaker(current_equity) {
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
            } else {
                self.metrics.circuit_breaker_status.set(0.0);
            }
        }

        // Capture performance snapshot if monitor available
        if let Some(monitor) = &self.performance_monitor {
            for sym in self.current_prices.keys() {
                let _ = monitor.capture_snapshot(sym).await;
            }
        }

        Ok(())
    }

    /// Update volatility manager with latest ATR/Benchmark data (Non-blocking)
    pub async fn update_volatility(&self) -> anyhow::Result<()> {
        // Choose benchmark symbol based on asset class
        let benchmark = match self.asset_class {
            AssetClass::Crypto => "BTC/USDT",
            _ => "SPY",
        };

        let market_service = self.market_service.clone();
        let volatility_manager = self.volatility_manager.clone();
        let benchmark_string = benchmark.to_string();

        // Spawn background task to avoid blocking the event loop with network I/O
        tokio::spawn(async move {
            let now = Utc::now();
            let start = now - chrono::Duration::days(30); // 30 days to get enough candles

            match market_service
                .get_historical_bars(&benchmark_string, start, now, "1D")
                .await
            {
                Ok(candles) => {
                    if candles.len() < 2 {
                        return;
                    }

                    // Calculate proper True Range (TR)
                    // TR = Max(H-L, |H-Cp|, |L-Cp|)
                    // Computed in Decimal: no lossy f64 conversion, no zero fallback.
                    let last = &candles[candles.len() - 1];
                    let prev_close = candles[candles.len() - 2].close;

                    let tr = (last.high - last.low)
                        .max((last.high - prev_close).abs())
                        .max((last.low - prev_close).abs());

                    if tr > Decimal::ZERO {
                        let mut vm = volatility_manager.write().await;
                        vm.update(tr);
                        debug!(
                            "RiskManager: Volatility updated for {}. Latest TR: {}, Avg: {}",
                            benchmark_string,
                            tr,
                            vm.get_average_volatility()
                        );
                    }
                }
                Err(e) => {
                    warn!("RiskManager: Failed to fetch volatility data: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Emergency liquidation of entire portfolio
    /// Delegates to LiquidationService for emergency liquidation logic
    #[instrument(skip(self))]
    pub(super) async fn liquidate_portfolio(&mut self, reason: &str) {
        // Delegate liquidation to LiquidationService
        self.liquidation_service
            .liquidate_portfolio(reason, &self.current_prices)
            .await;
    }

    /// Cleanup tentative filled orders and release reservations
    pub(super) async fn reconcile_pending_orders(&mut self, portfolio: &Portfolio) {
        let tokens = self.order_reconciler.reconcile_pending_orders(portfolio);

        // Release all reservation tokens in batch
        self.portfolio_state_manager
            .release_reservations(tokens)
            .await;
    }
}
