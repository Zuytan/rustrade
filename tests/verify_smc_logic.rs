use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rustrade::application::strategies::AnalysisContext;
use rustrade::application::strategies::SMCStrategy;
use rustrade::application::strategies::TradingStrategy;
use rustrade::domain::trading::types::Candle;
use std::collections::VecDeque;

fn create_candle(price: f64) -> Candle {
    Candle {
        symbol: "TEST".to_string(),
        open: Decimal::from_f64_retain(price).unwrap(),
        high: Decimal::from_f64_retain(price + 1.0).unwrap(),
        low: Decimal::from_f64_retain(price - 1.0).unwrap(),
        close: Decimal::from_f64_retain(price).unwrap(),
        volume: Decimal::new(1000, 0),
        timestamp: 1000,
    }
}

fn create_context(candles: VecDeque<Candle>) -> AnalysisContext {
    let price = candles.back().unwrap().close.to_f64().unwrap();
    AnalysisContext {
        symbol: "TEST".to_string(),
        current_price: candles.back().unwrap().close,
        price_f64: price,
        fast_sma: 0.0,
        slow_sma: 0.0,
        trend_sma: 0.0,
        rsi: 50.0,
        macd_value: 0.0,
        macd_signal: 0.0,
        macd_histogram: 0.0,
        last_macd_histogram: None,
        atr: 1.0,
        bb_upper: 0.0,
        bb_middle: 0.0,
        bb_lower: 0.0,
        adx: 0.0,
        has_position: false,
        timestamp: 1000,
        candles,
        rsi_history: VecDeque::new(),
        ofi_value: 0.5, // Strong OFI to pass filter
        cumulative_delta: 100.0,
        volume_profile: None,
        ofi_history: vec![0.5, 0.5, 0.5].into_iter().collect(),
        timeframe_features: None,
    }
}

#[test]
fn test_smc_signals_fresh_fvg_without_retracement() {
    let strategy = SMCStrategy::new(20, 0.001, 1.0);
    let mut candles = VecDeque::new();

    // Context
    for _ in 0..10 {
        candles.push_back(create_candle(100.0));
    }

    // FVG Formation
    // 1. Base
    candles.push_back(create_candle(100.0)); // H=101, L=99
    // 2. Impulse
    candles.push_back(create_candle(105.0)); // H=106, L=104
    // 3. Confirmation (Gap 101-104)
    candles.push_back(create_candle(108.0)); // H=109, L=107. Low3(107) > High1(101). Gap=6.0

    // Current price is 108.0. Gap is 101-104 (approx).
    // We are WAY above gap.

    let ctx = create_context(candles);
    let signal = strategy.analyze(&ctx);

    // If this returns Some(Buy), we are chasing.
    if let Some(sig) = signal {
        println!("Signal generated: {:?}", sig);
        panic!("SMC Strategy is chasing! It signaled BUY at price 108.0 when FVG is at 101-104");
    }
}
