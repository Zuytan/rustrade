//! Portfolio Valuation Service
//!
//! Handles portfolio valuation updates, pending order reconciliation, and volatility tracking.
//! Extracted from RiskManager to follow Single Responsibility Principle.

use crate::application::monitoring::portfolio_state_manager::PortfolioStateManager;
use crate::config::AssetClass;
use crate::domain::ports::MarketDataService;
use crate::domain::risk::volatility_manager::VolatilityManager;
use crate::domain::trading::portfolio::Portfolio;
use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Portfolio Valuation Service
///
/// # Responsibilities
///
/// - Update portfolio valuation with latest market prices
/// - Reconcile pending orders with portfolio state
/// - Update volatility metrics for risk calculations
pub struct PortfolioValuationService {
    market_service: Arc<dyn MarketDataService>,
    portfolio_state_manager: Arc<PortfolioStateManager>,
    volatility_manager: Arc<RwLock<VolatilityManager>>,
    asset_class: AssetClass,
}

impl PortfolioValuationService {
    /// Create a new PortfolioValuationService
    pub fn new(
        market_service: Arc<dyn MarketDataService>,
        portfolio_state_manager: Arc<PortfolioStateManager>,
        volatility_manager: Arc<RwLock<VolatilityManager>>,
        asset_class: AssetClass,
    ) -> Self {
        Self {
            market_service,
            portfolio_state_manager,
            volatility_manager,
            asset_class,
        }
    }

    /// Fetch latest prices for all held positions and update cache
    pub async fn update_portfolio_valuation(
        &self,
        current_prices: &mut HashMap<String, Decimal>,
    ) -> Result<(Portfolio, Decimal)> {
        // 1. Get fresh portfolio snapshot
        let snapshot = self.portfolio_state_manager.refresh().await?;

        // 2. Collect symbols
        let symbols: Vec<String> = snapshot.portfolio.positions.keys().cloned().collect();
        if symbols.is_empty() {
            let cash = snapshot.portfolio.cash;
            return Ok((snapshot.portfolio, cash));
        }

        // 3. Fetch latest prices
        match self.market_service.get_prices(symbols).await {
            Ok(prices) => {
                // Update cache
                for (sym, price) in prices {
                    current_prices.insert(sym, price);
                }

                // 4. Calculate Equity with NEW prices
                let current_equity = snapshot.portfolio.total_equity(current_prices);

                Ok((snapshot.portfolio, current_equity))
            }
            Err(e) => {
                warn!("PortfolioValuationService: Failed to update prices: {}", e);
                Err(e)
            }
        }
    }

    /// Update volatility manager with latest ATR/Benchmark data
    pub async fn update_volatility(&self) -> Result<()> {
        // Choose benchmark symbol based on asset class
        let benchmark = match self.asset_class {
            AssetClass::Crypto => "BTC/USDT",
            _ => "SPY",
        };

        // Fetch last 30 days for ATR calculation
        let now = Utc::now();
        let start = now - chrono::Duration::days(30);

        match self
            .market_service
            .get_historical_bars(benchmark, start, now, "1D")
            .await
        {
            Ok(candles) => {
                if candles.len() < 2 {
                    return Ok(());
                }

                // Calculate True Range for latest candle
                let last = &candles[candles.len() - 1];
                let high = last.high.to_f64().unwrap_or(0.0);
                let low = last.low.to_f64().unwrap_or(0.0);
                let range = high - low;

                if range > 0.0 {
                    let mut vm = self.volatility_manager.write().await;
                    vm.update(range);
                    debug!(
                        "PortfolioValuationService: Volatility updated for {}. Range: {:.2}, Avg: {:.2}",
                        benchmark,
                        range,
                        vm.get_average_volatility()
                    );
                }
            }
            Err(e) => {
                warn!(
                    "PortfolioValuationService: Failed to fetch volatility data: {}",
                    e
                );
            }
        }

        Ok(())
    }
}
