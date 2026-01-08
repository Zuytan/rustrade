use rust_decimal_macros::dec;
use rustrade::application::agents::analyst::{Analyst, AnalystConfig, AnalystDependencies};
use rustrade::application::strategies::{AnalysisContext, Signal, TradingStrategy};
use rustrade::domain::trading::portfolio::{Portfolio, Position};
use rustrade::domain::trading::types::{MarketEvent, OrderSide};
use rustrade::infrastructure::mock::{MockExecutionService, MockMarketDataService};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// Mock Strategy that buys first, then sells
struct SwitchingStrategy {
    first_call: AtomicBool,
}

impl SwitchingStrategy {
    fn new() -> Self {
        Self {
            first_call: AtomicBool::new(true),
        }
    }
}

impl TradingStrategy for SwitchingStrategy {
    fn name(&self) -> &str {
        "SwitchingStrategy"
    }

    fn analyze(&self, _ctx: &AnalysisContext) -> Option<Signal> {
        if self.first_call.fetch_and(false, Ordering::SeqCst) {
            Some(Signal::buy("Mock Buy"))
        } else {
            Some(Signal::sell("Mock Sell"))
        }
    }
}

#[tokio::test]
async fn test_sell_signal_suppression() {
    // 1. Setup
    let (market_tx, market_rx) = mpsc::channel(100);
    let (proposal_tx, mut proposal_rx) = mpsc::channel(100);

    // Start with NO position to allow Buy
    let portfolio = Arc::new(RwLock::new(Portfolio::new()));
    portfolio.write().await.cash = dec!(100000);

    let execution_service = Arc::new(MockExecutionService::new(portfolio.clone()));
    let market_service = Arc::new(MockMarketDataService::new_no_sim());

    let config = AnalystConfig {
        min_hold_time_minutes: 0,
        order_cooldown_seconds: 0,
        ..Default::default()
    };

    let dependencies = AnalystDependencies {
        execution_service: execution_service.clone(),
        market_service: market_service.clone(),
        candle_repository: None,
        strategy_repository: None,
        spread_cache: Arc::new(
            rustrade::application::market_data::spread_cache::SpreadCache::new(),
        ),
        win_rate_provider: None,
        ui_candle_tx: None,
    };

    let (_analyst_cmd_tx, analyst_cmd_rx) = mpsc::channel(10);
    let analyst = Analyst::new(
        market_rx,
        analyst_cmd_rx,
        proposal_tx,
        config,
        Arc::new(SwitchingStrategy::new()),
        dependencies,
    );

    let _handle = tokio::spawn(async move {
        let mut a = analyst;
        a.run().await;
    });

    use rustrade::domain::trading::types::Candle;

    // ...

    // 2. Trigger BUY (Phase 1)
    market_tx
        .send(MarketEvent::Candle(Candle {
            symbol: "AAPL".to_string(),
            open: dec!(150),
            high: dec!(150),
            low: dec!(150),
            close: dec!(150),
            volume: 100.0,
            timestamp: 1000,
        }))
        .await
        .unwrap();

    let prop = proposal_rx.recv().await.expect("Should get Buy proposal");
    assert_eq!(prop.side, OrderSide::Buy);
    println!("Received Buy Proposal");

    // 3. Simulate Execution & ACK
    // Update portfolio to have position
    {
        let mut pf = portfolio.write().await;
        pf.positions.insert(
            "AAPL".to_string(),
            Position {
                symbol: "AAPL".to_string(),
                quantity: dec!(10),
                average_price: dec!(150),
            },
        );
    }

    // 4. Trigger SELL (Phase 2)
    // Send new candle.

    market_tx
        .send(MarketEvent::Candle(Candle {
            symbol: "AAPL".to_string(),
            open: dec!(155),
            high: dec!(155),
            low: dec!(155),
            close: dec!(155),
            volume: 100.0,
            timestamp: 2000,
        }))
        .await
        .unwrap();

    // We wait for proposal. If suppressed, we get NOTHING.
    // Use timeout.
    let res = tokio::time::timeout(std::time::Duration::from_secs(2), proposal_rx.recv()).await;

    if let Ok(Some(prop)) = res {
        println!("Received Proposal: {:?}", prop);
        assert_eq!(prop.side, OrderSide::Sell, "Unexpected proposal side");
        panic!("Test Failed: Sell Proposal was NOT suppressed! (Did T-Stop fail to activate?)");
    } else {
        println!("Confirmed: Sell Proposal suppressed by Trailing Stop.");
    }
}
