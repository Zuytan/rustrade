use rust_decimal_macros::dec;

use rust_decimal::Decimal;
use rustrade::application::monitoring::connection_health_service::{
    ConnectionHealthService, ConnectionStatus,
};
use rustrade::application::system::Application;
use rustrade::config::{Config, Mode};
use rustrade::domain::ports::ExecutionService;
use rustrade::domain::trading::types::Candle;
use rustrade::domain::trading::types::{MarketEvent, OrderSide};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use rustrade::infrastructure::observability::Metrics;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

async fn create_online_health_service() -> Arc<ConnectionHealthService> {
    let svc = Arc::new(ConnectionHealthService::new());
    svc.set_market_data_status(ConnectionStatus::Online, None)
        .await;
    svc
}

#[tokio::test]
async fn test_e2e_golden_cross_buy() -> anyhow::Result<()> {
    // Setup logging to see output with --nocapture
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();

    // 1. Setup Config (Mock Mode)
    let config = Config {
        mode: Mode::Mock,
        asset_class: rustrade::config::AssetClass::Stock,
        broker: rustrade::config::BrokerEnvConfig::default(),
        strategy: rustrade::domain::config::StrategyConfig {
            strategy_mode: rustrade::domain::market::strategy_config::StrategyMode::SMC,
            fast_sma_period: 2,
            slow_sma_period: 5,
            sma_threshold: dec!(0.001),
            rsi_threshold: dec!(99.0),
            ..rustrade::domain::config::StrategyConfig::default()
        },
        risk: rustrade::domain::config::RiskConfig {
            max_positions: 1,
            trade_quantity: Decimal::from(1),
            order_cooldown_seconds: 0,
            risk_per_trade_percent: dec!(0.01),
            ..rustrade::domain::config::RiskConfig::default()
        },
        platform: rustrade::config::PlatformConfig {
            symbols: vec!["BTC/USD".to_string()],
            spread_bps: dec!(0.0),
            min_profit_ratio: dec!(0.0),
            ..rustrade::config::PlatformConfig::default()
        },
        observability: rustrade::config::ObservabilityEnvConfig {
            enabled: false,
            ..rustrade::config::ObservabilityEnvConfig::default()
        },
        simulation: rustrade::config::SimulationEnvConfig {
            enabled: false,
            ..rustrade::config::SimulationEnvConfig::default()
        },
    };

    // 2. Build Application
    let _app = Application::build(config.clone()).await?;

    // 3. Get services to interact with
    // We need to downcast or access known types.
    // Since app.market_service is Arc<dyn MarketDataService>, we need to know it's MockMarketDataService.
    // Rust doesn't support easy downcasting of Arc<dyn Trait> unless we implemented Any.
    // However, `MockMarketDataService` struct definition is available.
    // A trick: We created the app, we know it's mock.
    // BUT we stored them as trait objects.
    // We might need to unsafe cast or just instantiate services externally and pass them?
    // `Application` owns them.
    // Refactoring `Application` to allow injecting services would be best, but `build` creates them.
    // Let's see if we can trick it or if we should add a helper to `Application` for testing?
    // "downcast_ref" works if trait extends Any. `MarketDataService` likely doesn't.
    //
    // Quick fix: Re-implement `MockMarketDataService` to use a global/static or shared state that we can access from outside?
    // Better: Allow `Application` to return the concrete types if we made them generic? No.
    //
    // Simplest: Check if we can change `Application` to have public fields and just hope `Any` works or
    // modify `MarketDataService` trait to have `as_any`.

    // Let's rely on the fact that we can't easily downcast.
    // Modified Plan: Modify `MarketDataService` trait to include `as_any` or specific testing hook?
    // OR: Modify `Application::build` to take services as optional args?
    // OR: Just construct `Application` fields manually in test and skip `Application::build`?
    // `Application` fields are public!

    // We can just instantiate the services locally, then construct `Application` struct manually!
    #[derive(Clone)]
    struct NullRiskStateRepository;
    #[async_trait::async_trait]
    impl rustrade::domain::repositories::RiskStateRepository for NullRiskStateRepository {
        async fn save(
            &self,
            _state: &rustrade::domain::risk::state::RiskState,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn load(
            &self,
            _id: &str,
        ) -> anyhow::Result<Option<rustrade::domain::risk::state::RiskState>> {
            Ok(None)
        }
    }
    let null_risk_state = std::sync::Arc::new(NullRiskStateRepository);

    let portfolio = std::sync::Arc::new(tokio::sync::RwLock::new(
        rustrade::domain::trading::portfolio::Portfolio::new(),
    ));
    portfolio.write().await.cash = Decimal::from(100_000);

    let mock_market = std::sync::Arc::new(MockMarketDataService::new_no_sim());
    let mock_execution = std::sync::Arc::new(MockExecutionService::new(portfolio.clone()));
    let null_trade_repo = std::sync::Arc::new(rustrade::infrastructure::mock::NullTradeRepository);
    let _null_candle_repo =
        std::sync::Arc::new(rustrade::infrastructure::mock::NullCandleRepository);

    let null_strategy_repo =
        std::sync::Arc::new(rustrade::infrastructure::mock::NullStrategyRepository);

    // --- Persistence Setup (In-Memory for Test) ---
    // We need a real PersistenceHandle for the Application struct, even if we override fields.
    // Use in-memory SQLite for speed and isolation.
    let db =
        rustrade::infrastructure::persistence::database::Database::new("sqlite::memory:").await?;

    // Create concrete repositories needed for PersistenceHandle
    let candle_repo = std::sync::Arc::new(
        rustrade::infrastructure::persistence::repositories::SqliteCandleRepository::new(
            db.pool.clone(),
        ),
    );
    let order_repo = std::sync::Arc::new(
        rustrade::infrastructure::persistence::repositories::SqliteOrderRepository::new(
            db.pool.clone(),
        ),
    );
    let strategy_repo = std::sync::Arc::new(
        rustrade::infrastructure::persistence::repositories::SqliteStrategyRepository::new(
            db.pool.clone(),
        ),
    );
    let risk_state_repo = std::sync::Arc::new(
        rustrade::infrastructure::persistence::repositories::SqliteRiskStateRepository::new(
            db.clone(),
        ),
    );
    let opt_history_repo = std::sync::Arc::new(
        rustrade::infrastructure::persistence::repositories::SqliteOptimizationHistoryRepository::new(db.pool.clone())
    );
    let snapshot_repo = std::sync::Arc::new(
        rustrade::infrastructure::persistence::repositories::SqlitePerformanceSnapshotRepository::new(db.pool.clone())
    );
    let trigger_repo = std::sync::Arc::new(
        rustrade::infrastructure::persistence::repositories::SqliteReoptimizationTriggerRepository::new(db.pool.clone())
    );

    let persistence = rustrade::application::bootstrap::persistence::PersistenceHandle {
        db,
        candle_repository: candle_repo.clone(), // Use real repo for handle, but app might use null if we override?
        // Actually, the test was passing None or Null before.
        // If we want minimal changes, we just satisfy the struct.
        order_repository: order_repo.clone(),
        strategy_repository: strategy_repo.clone(),
        risk_state_repository: risk_state_repo.clone(),
        opt_history_repo,
        snapshot_repo,
        trigger_repo,
    };

    // --- Services Setup ---
    let spread_cache =
        std::sync::Arc::new(rustrade::application::market_data::spread_cache::SpreadCache::new());

    // We construct the ServicesHandle with our Mocks
    let services = rustrade::application::bootstrap::services::ServicesHandle {
        market_service: mock_market.clone(),
        execution_service: mock_execution.clone(),
        spread_cache: spread_cache.clone(),
        adaptive_optimization_service: None,
        performance_monitor: None,
        connection_health_service: create_online_health_service().await,
        metrics: Metrics::default(),
    };

    let app = Application {
        config,
        market_service: mock_market.clone(),
        execution_service: mock_execution.clone(),
        portfolio: portfolio.clone(),
        order_repository: null_trade_repo, // We keep using null/mocks for what the test explicitly mocked?
        // Actually, using the real in-memory repo matches the new PersistenceHandle better,
        // but if the test relies on specific mock behavior (like null repo doing nothing), we should keep it.
        // However, `app.start()` uses `app.persistence.order_repository` internally to init agents!
        // `app.order_repository` is just a reference kept on App struct.
        // If `app.start()` uses `persistence`, then `persistence.order_repository` IS the one that will be used by agents.
        // The test overrides `app.order_repository` but that might be ignored by `AgentsBootstrap` which looks at `persistence`.

        // CRITICAL FIX: The `Application::start` constructs agents using `self.persistence`.
        // So injecting `NullTradeRepository` into `app.order_repository` field is USELESS if agents use `persistence`.
        // The test probably wants `MockExecutionService` to be used.
        // `AgentsBootstrap` uses `services.execution_service`. We populated that with `mock_execution`. Good.
        // `AgentsBootstrap` uses `persistence.order_repository` for `Executor`?
        // Let's check `bootstrap/agents.rs`.
        // `let mut executor = Executor::new(..., Some(persistence.order_repository.clone())`.
        // So `Executor` will use the Real Sqlite Repo (In-Memory).
        // This is fine! It's actually better than Null.
        candle_repository: None,
        strategy_repository: null_strategy_repo,
        adaptive_optimization_service: None,
        performance_monitor: None,
        spread_cache: spread_cache.clone(),
        risk_state_repository: null_risk_state, // Again, this field is likely ignored by start() which uses persistence
        connection_health_service: services.connection_health_service.clone(),
        metrics: services.metrics.clone(),
        persistence,
        services,
        agent_registry: std::sync::Arc::new(
            rustrade::application::monitoring::agent_status::AgentStatusRegistry::new(
                rustrade::infrastructure::observability::Metrics::new().unwrap(),
            ),
        ),
    };

    // 4. Run Application (BACKGROUND)
    tokio::spawn(async move {
        app.start().await.unwrap();
    });

    // Wait for agents to start
    sleep(Duration::from_millis(100)).await;

    // 5. Inject Data (Golden Cross Scenario)
    // Strategy: Fast SMA (2) crosses ABOVE Slow SMA (5).
    // We need enough data points to compute SMAs.
    // Periods: 5. So we need at least 5 points.

    // Initial State: Price Flat or downtrend.
    // P1: 100
    // P2: 100
    // P3: 100
    // P4: 100
    // P5: 100 -> Fast=100, Slow=100.

    // Upward trend to cross.
    // P6: 110 -> Fast=(100+110)/2 = 105. Slow=(100+100+100+100+110)/5 = 102.
    // CROSSOVER! 105 > 102.

    let symbol = "BTC/USD".to_string();

    // Scenario:
    // 1. Establish Baseline (100)
    // 2. Dip to trigger "Below" state (Fast < Slow)
    // 3. Rip to trigger "Above" state (Fast > Slow) -> BUY SIGNAL

    // Scenario: SMC Bullish Sequence
    let smc_data = [
        (100.0, 101.0, 99.0, 99.0),   // C1: Bearish OB
        (99.0, 104.0, 99.0, 104.0),   // C2: Impulsive Bullish
        (104.0, 108.0, 103.0, 108.0), // C3: FVG Top
        (108.0, 108.0, 102.0, 102.0), // C4: Retracement
        (102.0, 105.0, 102.0, 105.0), // C5: Bullish Confirm
    ];

    let start_time = chrono::Utc::now();
    for (i, &(o, h, l, c)) in smc_data.iter().enumerate() {
        let timestamp = start_time + chrono::Duration::seconds(60 * (i as i64 + 1));
        mock_market
            .publish(MarketEvent::Candle(Candle {
                symbol: symbol.clone(),
                open: Decimal::from_f64_retain(o).unwrap(),
                high: Decimal::from_f64_retain(h).unwrap(),
                low: Decimal::from_f64_retain(l).unwrap(),
                close: Decimal::from_f64_retain(c).unwrap(),
                volume: Decimal::new(100, 0),
                timestamp: timestamp.timestamp_millis(),
            }))
            .await;
        sleep(Duration::from_millis(10)).await;
    }

    // Flush the aggregator by sending one more event in the future
    let flush_timestamp = start_time + chrono::Duration::seconds(60 * (smc_data.len() as i64 + 5));
    mock_market
        .publish(MarketEvent::Quote {
            symbol: symbol.clone(),
            price: Decimal::from(111),
            quantity: Decimal::from(100),
            timestamp: flush_timestamp.timestamp_millis(),
        })
        .await;
    sleep(Duration::from_millis(100)).await;

    sleep(Duration::from_secs(1)).await;

    // 6. Verify Execution
    // Check if an order was placed
    let orders = mock_execution.get_today_orders().await?;
    assert!(!orders.is_empty(), "Should have placed an order");

    let order = &orders[0];
    assert_eq!(order.symbol, symbol);
    assert!(matches!(order.side, OrderSide::Buy));
    // assert_eq!(order.quantity, config.trade_quantity); // Analyst uses risk-based sizing
    assert!(
        order.quantity > Decimal::ZERO,
        "Quantity should be positive"
    );

    Ok(())
}
