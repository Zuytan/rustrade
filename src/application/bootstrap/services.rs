use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::domain::ports::{ExecutionService, MarketDataService};
// Unused imports removed
use crate::application::bootstrap::persistence::PersistenceHandle;
use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::performance_monitoring_service::PerformanceMonitoringService;
use crate::application::optimization::{
    adaptive_optimization_service::AdaptiveOptimizationService,
    optimizer::{GridSearchOptimizer, ParameterGrid},
};
use crate::domain::performance::performance_evaluator::{
    EvaluationThresholds, PerformanceEvaluator,
};
use crate::domain::trading::portfolio::Portfolio;
use crate::infrastructure::factory::ServiceFactory;
use crate::infrastructure::mock::MockExecutionService;

pub struct ServicesHandle {
    pub market_service: Arc<dyn MarketDataService>,
    pub execution_service: Arc<dyn ExecutionService>,
    pub spread_cache: Arc<SpreadCache>,
    pub adaptive_optimization_service: Option<Arc<AdaptiveOptimizationService>>,
    pub performance_monitor: Option<Arc<PerformanceMonitoringService>>,
}

pub struct ServicesBootstrap;

impl ServicesBootstrap {
    pub async fn init(
        config: &Config,
        persistence: &PersistenceHandle,
        portfolio: Arc<RwLock<Portfolio>>,
    ) -> Result<ServicesHandle> {
        // 1. Initialize Infrastructure Services (Using Factory)
        let (market_service, execution_service, spread_cache) = ServiceFactory::create_services(
            config,
            Some(persistence.candle_repository.clone()),
            portfolio.clone(),
        );

        // 2. Initialize Adaptive Optimization Services
        let performance_monitor = if config.adaptive_optimization_enabled {
            Some(Arc::new(PerformanceMonitoringService::new(
                persistence.snapshot_repo.clone(),
                persistence.candle_repository.clone(),
                market_service.clone(),
                portfolio.clone(),
                persistence.order_repository.clone(),
                config.regime_detection_window,
            )))
        } else {
            None
        };

        let adaptive_optimization_service = if config.adaptive_optimization_enabled {
            let initial_cash = rust_decimal::Decimal::from(100_000); // Default for optimization simulation
            let execution_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync> =
                Arc::new(move || {
                    let portfolio = Arc::new(RwLock::new({
                        let mut p = Portfolio::new();
                        p.cash = initial_cash;
                        p
                    }));
                    Arc::new(MockExecutionService::new(portfolio))
                });

            let optimizer = Arc::new(GridSearchOptimizer::new(
                market_service.clone(),
                execution_factory,
                ParameterGrid::default(), // Load from file in real world
                config.strategy_mode,
                config.min_profit_ratio, // Use config value
            ));

            Some(Arc::new(AdaptiveOptimizationService::new(
                optimizer,
                persistence.opt_history_repo.clone(),
                persistence.snapshot_repo.clone(),
                persistence.trigger_repo.clone(),
                persistence.strategy_repository.clone(),
                persistence.candle_repository.clone(),
                PerformanceEvaluator::new(EvaluationThresholds::default()),
                config.regime_detection_window,
                true,
            )))
        } else {
            None
        };

        Ok(ServicesHandle {
            market_service,
            execution_service,
            spread_cache,
            adaptive_optimization_service,
            performance_monitor,
        })
    }
}
