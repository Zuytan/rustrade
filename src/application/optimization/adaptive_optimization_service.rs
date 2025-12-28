use crate::application::optimization::optimizer::GridSearchOptimizer;
use crate::domain::market::market_regime::{MarketRegimeType, MarketRegimeDetector};
use crate::domain::optimization::optimization_history::OptimizationHistory;
use crate::domain::performance::performance_evaluator::PerformanceEvaluator;
use crate::domain::optimization::reoptimization_trigger::{ReoptimizationTrigger, TriggerReason};
use crate::domain::repositories::{
    CandleRepository, OptimizationHistoryRepository, PerformanceSnapshotRepository, 
    ReoptimizationTriggerRepository, StrategyRepository
};
use crate::domain::market::strategy_config::{StrategyDefinition, StrategyMode};
use anyhow::Result;
use chrono::{Duration, Utc};
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct AdaptiveOptimizationService {
    optimizer: Arc<GridSearchOptimizer>,
    history_repo: Arc<dyn OptimizationHistoryRepository>,
    snapshot_repo: Arc<dyn PerformanceSnapshotRepository>,
    trigger_repo: Arc<dyn ReoptimizationTriggerRepository>,
    strategy_repo: Arc<dyn StrategyRepository>,
    candle_repo: Arc<dyn CandleRepository>,
    evaluator: PerformanceEvaluator,
    regime_detector: MarketRegimeDetector,
    enabled: bool,
}

impl AdaptiveOptimizationService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        optimizer: Arc<GridSearchOptimizer>,
        history_repo: Arc<dyn OptimizationHistoryRepository>,
        snapshot_repo: Arc<dyn PerformanceSnapshotRepository>,
        trigger_repo: Arc<dyn ReoptimizationTriggerRepository>,
        strategy_repo: Arc<dyn StrategyRepository>,
        candle_repo: Arc<dyn CandleRepository>,
        evaluator: PerformanceEvaluator,
        regime_window: usize,
        enabled: bool,
    ) -> Self {
        Self {
            optimizer,
            history_repo,
            snapshot_repo,
            trigger_repo,
            strategy_repo,
            candle_repo,
            evaluator,
            regime_detector: MarketRegimeDetector::new(regime_window, 25.0, 2.0), // TODO: Config
            enabled,
        }
    }

    /// Primary entry point: Run daily evaluation to see if we need to re-optimize
    pub async fn run_daily_evaluation(&self, symbol: &str) -> Result<()> {
        if !self.enabled {
            info!("Adaptive optimization disabled, skipping evaluation for {}", symbol);
            return Ok(());
        }

        info!("Running daily adaptive optimization evaluation for {}", symbol);

        // 1. Get latest performance snapshot
        let snapshot = self.snapshot_repo.get_latest(symbol).await?;
        
        // 2. Evaluate performance
        if let Some(snap) = snapshot {
            if let Some(reason) = self.evaluator.evaluate(&snap) {
                warn!("Triggering re-optimization for {} due to: {}", symbol, reason);
                self.trigger_reoptimization(symbol, reason).await?;
            } else {
                // Check for regime change even if performance is okay
                // Fetch recent candles
                let end_ts = Utc::now().timestamp();
                let start_ts = end_ts - (30 * 24 * 60 * 60); 
                let candles = self.candle_repo.get_range(symbol, start_ts, end_ts).await?;
                
                let current_regime = self.regime_detector.detect(&candles)?;
                let last_opt = self.history_repo.get_latest_active(symbol).await?;

                if let Some(last) = last_opt {
                    if last.market_regime != current_regime.regime_type 
                       && current_regime.regime_type != MarketRegimeType::Unknown 
                       && current_regime.confidence > 0.7 
                    {
                        warn!("Triggering re-optimization for {} due to Regime Change: {} -> {}", 
                            symbol, last.market_regime, current_regime.regime_type);
                        self.trigger_reoptimization(symbol, TriggerReason::RegimeChange).await?;
                    }
                } else {
                     // No history, maybe first run?
                     info!("No active optimization found for {}, considering initial optimization", symbol);
                }
            }
        } else {
            warn!("No performance snapshot available for {}", symbol);
        }

        Ok(())
    }

    pub async fn trigger_reoptimization(&self, symbol: &str, reason: TriggerReason) -> Result<()> {
        // Idempotency check: don't create checks if one is pending
        let pending = self.trigger_repo.get_pending().await?;
        if pending.iter().any(|t| t.symbol == symbol) {
            info!("Re-optimization already pending for {}", symbol);
            return Ok(());
        }

        let trigger = ReoptimizationTrigger::new(symbol.to_string(), reason);
        self.trigger_repo.save(&trigger).await?;
        
        // Execute immediately for now (could be async job)
        self.execute_reoptimization(symbol, &trigger).await?;

        Ok(())
    }

    async fn execute_reoptimization(&self, symbol: &str, _trigger: &ReoptimizationTrigger) -> Result<()> {
        info!("Executing re-optimization for {}", symbol);
        
        // Mark trigger running (if we had IDs returned from save, currently we re-query or assume)
        // For MVP we just run it.

        let end_date = Utc::now();
        let start_date = end_date - Duration::days(90); // Optimize on last quarter

        // Run Grid Search
        let results = self.optimizer.run_optimization(symbol, start_date, end_date).await?;
        let top_results = self.optimizer.rank_results(results, 1);

        if let Some(best) = top_results.first() {
            info!("Found new optimal parameters for {}: Sharpe={}", symbol, best.sharpe_ratio);

            // Serialize config
            let config_json = serde_json::to_string(&best.params)?;
            let metrics_json = serde_json::to_string(&best)?;

            // Determine regime of the optimization period
            let candles = self.candle_repo.get_range(symbol, start_date.timestamp(), end_date.timestamp()).await?;
            let regime = self.regime_detector.detect(&candles)?;

            // Save History
            // Deactivate old
            self.history_repo.deactivate_old(symbol).await?;
            
            let history = OptimizationHistory::new(
                symbol.to_string(),
                config_json.clone(),
                metrics_json,
                regime.regime_type,
                best.sharpe_ratio,
                best.total_return,
                best.win_rate,
            );
            self.history_repo.save(&history).await?;

            // Update Active Strategy
            let strategy_def = StrategyDefinition {
                symbol: symbol.to_string(),
                mode: StrategyMode::Advanced, // Assuming Advanced for now
                config_json,
                is_active: true,
            };
            self.strategy_repo.save(&strategy_def).await?;
            
            info!("Successfully applied new parameters for {}", symbol);
        } else {
            error!("Optimization failed to produce result for {}", symbol);
        }

        Ok(())
    }
}
