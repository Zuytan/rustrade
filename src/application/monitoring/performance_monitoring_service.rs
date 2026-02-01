use crate::application::monitoring::connection_health_service::ConnectionHealthService;
use crate::domain::market::market_regime::MarketRegimeDetector;
use crate::domain::performance::calculator;
use crate::domain::performance::performance_snapshot::PerformanceSnapshot;
use crate::domain::ports::MarketDataService;
use crate::domain::repositories::TradeRepository;
use crate::domain::repositories::{CandleRepository, PerformanceSnapshotRepository};
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::Order;
use anyhow::Result;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

pub struct PerformanceMonitoringService {
    snapshot_repository: Arc<dyn PerformanceSnapshotRepository>,
    candle_repository: Arc<dyn CandleRepository>,
    market_service: Arc<dyn MarketDataService>,
    regime_detector: MarketRegimeDetector,

    portfolio: Arc<RwLock<Portfolio>>,
    trade_repository: Arc<dyn TradeRepository>,
    #[allow(dead_code)] // Will be used in future metrics exposure
    connection_health_service: Arc<ConnectionHealthService>,
}

impl PerformanceMonitoringService {
    pub fn new(
        snapshot_repository: Arc<dyn PerformanceSnapshotRepository>,
        candle_repository: Arc<dyn CandleRepository>,
        market_service: Arc<dyn MarketDataService>,

        portfolio: Arc<RwLock<Portfolio>>,
        trade_repository: Arc<dyn TradeRepository>,
        connection_health_service: Arc<ConnectionHealthService>,
        regime_window_size: usize,
    ) -> Self {
        Self {
            snapshot_repository,
            candle_repository,
            market_service,
            regime_detector: MarketRegimeDetector::new(regime_window_size, dec!(25.0), dec!(2.0)), // Defaults, should come from config
            portfolio,
            trade_repository,
            connection_health_service,
        }
    }

    pub async fn capture_snapshot(&self, symbol: &str) -> Result<()> {
        let portfolio = self.portfolio.read().await;

        // Fetch current prices for valuation
        let symbols: Vec<String> = portfolio.positions.keys().cloned().collect();
        let prices = self
            .market_service
            .get_prices(symbols)
            .await
            .unwrap_or_default();

        // Calculate basic metrics from portfolio
        let equity = portfolio.total_equity(&prices);
        let starting_cash = portfolio.starting_cash;

        let drawdown_pct = if starting_cash > rust_decimal::Decimal::ZERO {
            let max_equity = portfolio.max_equity.max(starting_cash); // Ensure we account for starting point
            if max_equity > rust_decimal::Decimal::ZERO {
                (max_equity - equity) / max_equity
            } else {
                rust_decimal::Decimal::ZERO
            }
        } else {
            rust_decimal::Decimal::ZERO
        };

        // Detect current regime
        let end_ts = chrono::Utc::now().timestamp();
        let start_ts = end_ts - (30 * 24 * 60 * 60); // Last 30 days for candle data if needed, or just enough for window

        // We need enough candles for the detector window
        // This is a bit inefficient to fetch every time, in prod we'd cache recent candles
        let candles = self
            .candle_repository
            .get_range(symbol, start_ts, end_ts)
            .await?;
        let market_regime = self.regime_detector.detect(&candles)?;

        // Calculate Rolling Metrics (simplified for now, would need trade history)
        // For MVP, we'll placeholders or fetch from TradeRepository if available
        // Calculate Rolling Metrics (30d)
        let (sharpe_30d, win_rate_30d) = self.calculate_rolling_metrics(symbol, 30).await;

        let snapshot = PerformanceSnapshot::new(
            symbol.to_string(),
            equity,
            drawdown_pct.to_f64().unwrap_or(0.0),
            sharpe_30d,
            win_rate_30d,
            market_regime.regime_type,
        );

        self.snapshot_repository.save(&snapshot).await?;

        if market_regime.regime_type
            != crate::domain::market::market_regime::MarketRegimeType::Unknown
        {
            info!(
                "Performance Snapshot captured for {}: Regime={}, Equity={}",
                symbol, market_regime.regime_type, equity
            );
        }

        Ok(())
    }

    /// Calculate rolling performance metrics using FIFO LIFO matching on orders
    async fn calculate_rolling_metrics(&self, symbol: &str, days: i64) -> (f64, f64) {
        let trades = match self.trade_repository.find_by_symbol(symbol).await {
            Ok(t) => t,
            Err(e) => {
                warn!("Failed to fetch trades for metrics: {}", e);
                return (0.0, 0.0);
            }
        };

        // Filter by date (approximate timestamp check)
        let cutoff = chrono::Utc::now().timestamp() - (days * 24 * 3600);
        let relevant_orders: Vec<&Order> =
            trades.iter().filter(|t| t.timestamp >= cutoff).collect();

        if relevant_orders.is_empty() {
            return (0.0, 0.0);
        }

        // Delegate to domain utility
        calculator::calculate_metrics_from_orders(
            &relevant_orders.into_iter().cloned().collect::<Vec<_>>(),
        )
    }
}
