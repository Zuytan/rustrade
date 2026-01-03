use rustrade::application::agents::analyst::{Analyst, AnalystConfig};
use rustrade::domain::market::strategy_config::StrategyMode;
use rustrade::domain::repositories::CandleRepository;
use rustrade::domain::trading::types::{Candle, MarketEvent};

use rust_decimal::Decimal;
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rustrade::domain::trading::portfolio::Portfolio;
use tokio::sync::{mpsc, RwLock};

// --- Mock Candle Repository ---
#[derive(Clone)]
struct MockCandleRepo {
    candles: Arc<Mutex<Vec<Candle>>>,
}

impl MockCandleRepo {
    fn new() -> Self {
        Self {
            candles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn set_candles(&self, new_candles: Vec<Candle>) {
        let mut store = self.candles.lock().unwrap();
        *store = new_candles;
    }
}

#[async_trait]
impl CandleRepository for MockCandleRepo {
    async fn save(&self, _candle: &Candle) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_range(&self, _symbol: &str, start: i64, end: i64) -> anyhow::Result<Vec<Candle>> {
        let store = self.candles.lock().unwrap();
        // Simple filter
        let filtered: Vec<Candle> = store
            .iter()
            .filter(|c| c.timestamp >= start && c.timestamp <= end)
            .cloned()
            .collect();
        Ok(filtered)
    }

    async fn prune(&self, _days_retention: i64) -> anyhow::Result<u64> {
        Ok(0)
    }
}

// --- Helper to Generate Candles ---
fn generate_ranging_candles(symbol: &str, count: usize, start_ts: i64) -> Vec<Candle> {
    let mut candles = Vec::new();
    let base_price = 100.0;
    for i in 0..count {
        // Oscillate around 100
        let offset = (i as f64).sin() * 2.0;
        let close = base_price + offset;

        candles.push(Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(close - 0.5).unwrap(),
            high: Decimal::from_f64_retain(close + 0.5).unwrap(),
            low: Decimal::from_f64_retain(close - 0.5).unwrap(),
            close: Decimal::from_f64_retain(close).unwrap(),
            volume: 500.0, // Low volume for ranging
            timestamp: start_ts + (i as i64 * 86400),
        });
    }
    candles
}

#[tokio::test]
async fn test_adaptive_strategy_switching() {
    // Setup
    let (market_tx, market_rx) = mpsc::channel(100);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(100);

    let mut portfolio = Portfolio::new();
    portfolio.cash = Decimal::from(100000);
    let portfolio_lock = Arc::new(RwLock::new(portfolio));
    let exec_service = Arc::new(MockExecutionService::new(portfolio_lock));

    let repo = Arc::new(MockCandleRepo::new());

    let config = AnalystConfig {
        fast_sma_period: 2,
        slow_sma_period: 5,
        max_positions: 1,
        trade_quantity: Decimal::from(1),
        sma_threshold: 0.0,
        order_cooldown_seconds: 0,
        risk_per_trade_percent: 0.0,
        strategy_mode: StrategyMode::RegimeAdaptive, // <--- CRITICAL
        trend_sma_period: 20,
        rsi_period: 14,
        macd_fast_period: 12,
        macd_slow_period: 26,
        macd_signal_period: 9,
        trend_divergence_threshold: 0.005,
        trailing_stop_atr_multiplier: 3.0,
        atr_period: 14,
        rsi_threshold: 70.0,
        trend_riding_exit_buffer_pct: 0.02,
        mean_reversion_rsi_exit: 50.0,
        mean_reversion_bb_period: 20,
        slippage_pct: 0.0,
        commission_per_share: 0.0,
        max_position_size_pct: 0.1,
        bb_period: 20,
        bb_std_dev: 2.0,
        macd_fast: 12,
        macd_slow: 26,
        macd_signal: 9,
        ema_fast_period: 10,
        ema_slow_period: 30,
        take_profit_pct: 0.05,
        min_hold_time_minutes: 0,
        signal_confirmation_bars: 1,
        spread_bps: 5.0,
        min_profit_ratio: 2.0,
        macd_requires_rising: true,
        trend_tolerance_pct: 0.0,
        macd_min_threshold: 0.0,
        profit_target_multiplier: 1.5,
    };

    // Default strategy (initial) - usually Standard or DualSMA if factory defaults
    // But since config is RegimeAdaptive, Analyst resolves it.
    let strategy = Arc::new(
        rustrade::application::strategies::trend_riding::TrendRidingStrategy::new(2, 5, 0.0, 0.02),
    );

    let mut analyst = Analyst::new(
        market_rx,
        proposal_tx,
        config.clone(),
        strategy,
        rustrade::application::agents::analyst::AnalystDependencies {
            execution_service: exec_service,
            market_service: Arc::new(MockMarketDataService::new()),
            candle_repository: Some(repo.clone()),
            strategy_repository: None,
            win_rate_provider: None,
            spread_cache: std::sync::Arc::new(
                rustrade::application::market_data::spread_cache::SpreadCache::new(),
            ),
            ui_candle_tx: None,
        },
    );

    tokio::spawn(async move {
        analyst.run().await;
    });

    let symbol = "AAPL";
    let start_ts = 1600000000;

    // --- CASE 1: Ranging Market -> MeanReversion ---
    // Populate repo with ranging candles (30 days)
    let ranging_candles = generate_ranging_candles(symbol, 40, start_ts);
    repo.set_candles(ranging_candles.clone());

    // Send a new candle that triggers analysis
    // Make price effectively jump to trigger a signal if possible, or just regular update to check mode
    let trigger_candle_1 = Candle {
        symbol: symbol.to_string(),
        open: Decimal::from(100),
        high: Decimal::from(105),
        low: Decimal::from(40),   // Lower low
        close: Decimal::from(50), // Massive drop to ensure RSI < 30

        volume: 1000.0,

        timestamp: start_ts + (40 * 86400),
    };
    // Note: We need indicators to be calculated. The Analyst updates indicators from *Price Stream* (process_candle context.update).
    // But context.update buffers history locally?
    // `SymbolContext` uses `feature_service`. Feature service builds indicators from incoming checks.
    // DOES NOT load from repo!
    // So we need to feed enough candles to Analyst via `MarketEvent` to warm up indicators?
    // OR we rely on the fact that `Regime Detection` uses the REPO.
    // `Strategy Selection` depends on Regime.
    // So if Repo has Ranging data, `Regime` = Ranging.
    // Then `StrategySelector` picks `MeanReversion`.
    // Then `MeanReversion` strategy runs on `last_features`.
    // If `last_features` is empty (cold start), it won't signal.
    // But `Analyst` *log* "Adaptive Switch" or the proposal *Reason* will show the strategy!
    // Even if no signal, we want to know if it SWITCHED.
    // But we only get a Proposal if there is a signal.

    // To verify SWITCH alone, checking logs is best, but here we can only check Proposal output.
    // So we need to force a Signal.
    // `MeanReversion` signals Buy when Price < LowerBand (or RSI < 30).
    // If we feed 20 candles via MarketEvent, Bollinger Bands will form.

    // Feed ranging candles to warm up indicators
    for c in &ranging_candles {
        market_tx
            .send(MarketEvent::Candle(c.clone()))
            .await
            .unwrap();
        // Drain any warmup proposals
        while let Ok(_) = proposal_rx.try_recv() {}
    }

    // Send trigger candle that should generate MeanReversion buy signal
    market_tx
        .send(MarketEvent::Candle(trigger_candle_1.clone()))
        .await
        .unwrap();

    // Verify adaptive strategy switching works (Ranging -> MeanReversion)
    let prop1 = tokio::time::timeout(tokio::time::Duration::from_secs(5), proposal_rx.recv()).await;

    match prop1 {
        Ok(Some(p)) => {
            assert!(
                p.reason.contains("MeanReversion") || p.reason.contains("Ranging"),
                "Should use MeanReversion strategy in Ranging regime, got: {}",
                p.reason
            );
        }
        Ok(None) => panic!("Proposal channel closed unexpectedly"),
        Err(_) => panic!("Test timed out - no proposal generated for Ranging market"),
    }
}
