use crate::domain::market::market_regime::MarketRegimeDetector;
use crate::domain::performance::performance_snapshot::PerformanceSnapshot;
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::ports::MarketDataService;
use crate::domain::repositories::{CandleRepository, PerformanceSnapshotRepository};
use anyhow::Result;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub struct PerformanceMonitoringService {
    snapshot_repository: Arc<dyn PerformanceSnapshotRepository>,
    candle_repository: Arc<dyn CandleRepository>,
    market_service: Arc<dyn MarketDataService>,
    regime_detector: MarketRegimeDetector,
    portfolio: Arc<RwLock<Portfolio>>,
}

impl PerformanceMonitoringService {
    pub fn new(
        snapshot_repository: Arc<dyn PerformanceSnapshotRepository>,
        candle_repository: Arc<dyn CandleRepository>,
        market_service: Arc<dyn MarketDataService>,
        portfolio: Arc<RwLock<Portfolio>>,
        regime_window_size: usize,
    ) -> Self {
        Self {
            snapshot_repository,
            candle_repository,
            market_service,
            regime_detector: MarketRegimeDetector::new(regime_window_size, 25.0, 2.0), // Defaults, should come from config
            portfolio,
        }
    }

    pub async fn capture_snapshot(&self, symbol: &str) -> Result<()> {
        let portfolio = self.portfolio.read().await;
        
        // Fetch current prices for valuation
        let symbols: Vec<String> = portfolio.positions.keys().cloned().collect();
        let prices = self.market_service.get_prices(symbols).await.unwrap_or_default();

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
        let candles = self.candle_repository.get_range(symbol, start_ts, end_ts).await?;
        let market_regime = self.regime_detector.detect(&candles)?;

        // Calculate Rolling Metrics (simplified for now, would need trade history)
        // For MVP, we'll placeholders or fetch from TradeRepository if available
        let sharpe_30d = 0.0; // TODO: Implement rolling calculation
        let win_rate_30d = 0.0; // TODO: Implement rolling calculation

        let snapshot = PerformanceSnapshot::new(
            symbol.to_string(),
            equity,
            drawdown_pct.to_f64().unwrap_or(0.0),
            sharpe_30d,
            win_rate_30d,
            market_regime.regime_type,
        );

        self.snapshot_repository.save(&snapshot).await?;
        
        if market_regime.regime_type != crate::domain::market::market_regime::MarketRegimeType::Unknown {
             info!("Performance Snapshot captured for {}: Regime={}, Equity={}", 
                symbol, market_regime.regime_type, equity);
        }

        Ok(())
    }
}
