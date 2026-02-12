use super::*;
use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::trading::types::Candle;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal_macros::dec;
use std::collections::VecDeque;

/// QA Context Builder to easily create scenarios
struct ContextBuilder {
    ctx: AnalysisContext,
}

impl ContextBuilder {
    fn new(price: f64) -> Self {
        let d_price = Decimal::from_f64(price).unwrap();
        Self {
            ctx: AnalysisContext {
                symbol: "QA_TEST".to_string(),
                current_price: d_price,
                price_f64: price,
                fast_sma: Some(d_price),
                slow_sma: Some(d_price),
                trend_sma: Some(d_price),
                rsi: Some(dec!(50.0)),
                macd_value: Some(Decimal::ZERO),
                macd_signal: Some(Decimal::ZERO),
                macd_histogram: Some(Decimal::ZERO),
                last_macd_histogram: None,
                atr: Some(dec!(1.0)),
                bb_lower: Some(d_price * dec!(0.98)),
                bb_middle: Some(d_price),
                bb_upper: Some(d_price * dec!(1.02)),
                adx: Some(dec!(25.0)),
                has_position: false,
                position: None,
                timestamp: 100000,
                timeframe_features: None,
                candles: VecDeque::new(),
                rsi_history: VecDeque::new(),
                ofi_value: Decimal::ZERO,
                cumulative_delta: Decimal::ZERO,
                volume_profile: None,
                ofi_history: VecDeque::new(),
                hurst_exponent: None,
                skewness: None,
                momentum_normalized: None,
                realized_volatility: None,
                feature_set: None,
            },
        }
    }

    fn with_sma(mut self, fast: f64, slow: f64, trend: f64) -> Self {
        self.ctx.fast_sma = Some(Decimal::from_f64(fast).unwrap());
        self.ctx.slow_sma = Some(Decimal::from_f64(slow).unwrap());
        self.ctx.trend_sma = Some(Decimal::from_f64(trend).unwrap());
        self
    }

    fn with_rsi(mut self, rsi: f64) -> Self {
        self.ctx.rsi = Some(Decimal::from_f64(rsi).unwrap());
        self
    }

    fn with_adx(mut self, adx: f64) -> Self {
        self.ctx.adx = Some(Decimal::from_f64(adx).unwrap());
        self
    }

    fn with_macd(mut self, hist: f64) -> Self {
        self.ctx.macd_histogram = Some(Decimal::from_f64(hist).unwrap());
        self
    }

    fn with_position(mut self, has_pos: bool) -> Self {
        self.ctx.has_position = has_pos;
        self
    }

    fn with_candles(mut self, count: usize, price: f64) -> Self {
        for i in 0..count {
            let c = Candle {
                timestamp: 100000 - ((count - i) as i64 * 60),
                open: Decimal::from_f64(price).unwrap(),
                high: Decimal::from_f64(price).unwrap(),
                low: Decimal::from_f64(price).unwrap(),
                close: Decimal::from_f64(price).unwrap(),
                volume: dec!(1000.0),
                symbol: "QA_TEST".to_string(),
            };
            self.ctx.candles.push_back(c);
        }
        self
    }

    fn build(self) -> AnalysisContext {
        self.ctx
    }
}

// Factorized strategy provider
fn get_all_strategies() -> Vec<Box<dyn TradingStrategy>> {
    let strategies: Vec<Box<dyn TradingStrategy>> = vec![
        // Legacy
        Box::new(BreakoutStrategy::default()),
        Box::new(MomentumDivergenceStrategy::default()),
        Box::new(VWAPStrategy::default()),
        Box::new(DualSMAStrategy::new(20, 60, dec!(0.005))),
        Box::new(MeanReversionStrategy::new(20, dec!(70.0))),
        Box::new(TrendRidingStrategy::new(20, 60, dec!(0.005), dec!(0.02))),
        // Modern
        Box::new(OrderFlowStrategy::default()),
        Box::new(DynamicRegimeStrategy::with_config(
            DynamicRegimeConfig::default(),
        )),
        Box::new(StatisticalMomentumStrategy::default()),
        Box::new(ZScoreMeanReversionStrategy::default()),
        Box::new(SMCStrategy::default()),
        // Ensemble (Modern) - Using wrapper to box it
        Box::new(EnsembleStrategy::modern_ensemble(&AnalystConfig::default())),
    ];

    strategies
}

#[test]
fn test_qa_scenario_bull_market() {
    // Scenario: Strong Uptrend
    // Price > SMAs, RSI high but sustainable, ADX strong
    let ctx = ContextBuilder::new(105.0)
        .with_sma(103.0, 100.0, 95.0) // Fast > Slow > Trend
        .with_rsi(65.0)
        .with_adx(35.0) // Strong trend
        .with_macd(0.5) // Positive momentum
        .with_candles(100, 100.0) // History
        .with_position(false)
        .build();

    let strategies = get_all_strategies();

    for strategy in strategies {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| strategy.analyze(&ctx)));

        assert!(
            result.is_ok(),
            "Strategy {} panicked in Bull Market scenario",
            strategy.name()
        );

        if let Ok(Some(signal)) = result {
            // In a bull market, we expect BUY or HOLD (None).
            // Some mean reversion strategies might SELL if RSI is too high, which is valid logic.
            // But we mainly check for validity.
            assert!(
                signal.confidence > 0.0 && signal.confidence <= 1.0,
                "Strategy {} confidence out of bounds: {}",
                strategy.name(),
                signal.confidence
            );
            assert!(
                !signal.reason.is_empty(),
                "Strategy {} reason empty",
                strategy.name()
            );

            // VERIFICATION: Check if strategies supporting SL/TP are actually returning it
            // SMC, ZScoreMR, StatMomentum should return SL
            match strategy.name() {
                "SMC" | "ZScoreMR" | "StatMomentum" => {
                    if !signal.reason.contains("blocked") {
                        // Ignore if it was a blocked signal logging (though here we have Some(Signal))
                        assert!(
                            signal.suggested_stop_loss.is_some(),
                            "Strategy {} missing Stop Loss",
                            strategy.name()
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test_qa_scenario_bear_market() {
    // Scenario: Strong Downtrend
    // Price < SMAs, RSI low, ADX strong
    let ctx = ContextBuilder::new(95.0)
        .with_sma(97.0, 100.0, 105.0) // Fast < Slow < Trend
        .with_rsi(35.0)
        .with_adx(35.0)
        .with_macd(-0.5)
        .with_candles(100, 100.0)
        .with_position(true) // We have position to potentially sell
        .build();

    let strategies = get_all_strategies();

    for strategy in strategies {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| strategy.analyze(&ctx)));

        assert!(
            result.is_ok(),
            "Strategy {} panicked in Bear Market scenario",
            strategy.name()
        );
    }
}

#[test]
fn test_qa_scenario_flat_choppy() {
    // Scenario: Flat / Chop
    // Price ~ SMAs, Low ADX, RSI ~ 50
    let ctx = ContextBuilder::new(100.0)
        .with_sma(100.1, 100.0, 100.2) // Tangled
        .with_rsi(51.0)
        .with_adx(15.0) // Weak trend
        .with_macd(0.01)
        .with_candles(100, 100.0)
        .with_position(false)
        .build();

    let strategies = get_all_strategies();

    for strategy in strategies {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| strategy.analyze(&ctx)));

        assert!(
            result.is_ok(),
            "Strategy {} panicked in Flat Market scenario",
            strategy.name()
        );
    }
}

#[test]
fn test_qa_scenario_extreme_volatility() {
    // Scenario: Shock
    // Price moves 20% in one tick
    let ctx = ContextBuilder::new(120.0) // 20% jump from 100
        .with_sma(100.0, 100.0, 100.0)
        .with_rsi(95.0) // Extreme Overbought
        .with_adx(50.0)
        .with_macd(10.0)
        .with_candles(10, 100.0) // Short history
        .with_position(true)
        .build();

    let strategies = get_all_strategies();

    for strategy in strategies {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| strategy.analyze(&ctx)));

        assert!(
            result.is_ok(),
            "Strategy {} panicked in Extreme Volatility scenario",
            strategy.name()
        );
    }
}

#[test]
fn test_qa_scenario_insufficient_data() {
    // Scenario: No history
    let mut ctx = ContextBuilder::new(100.0).build();
    ctx.candles.clear(); // Ensure empty
    ctx.rsi_history.clear();

    let strategies = get_all_strategies();

    for strategy in strategies {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| strategy.analyze(&ctx)));

        assert!(
            result.is_ok(),
            "Strategy {} panicked with Insufficient Data",
            strategy.name()
        );
        // Most should return None
        if let Ok(Some(_)) = result {
            // Valid to return signal based on current price alone (though rare)
        }
    }
}

// -----------------------------------------------------------------------------
// Mathematical Precision Tests (QA Suite v2)
// -----------------------------------------------------------------------------

#[test]
fn test_precision_vwap() {
    // Objective: Verify VWAP is calculated exactly as Σ(TP * Vol) / Σ(Vol)
    // Manual Calc:
    // C1: High=101, Low=99, Close=100. TP=100. Vol=1000. TP*Vol=100,000.
    // C2: High=103, Low=101, Close=102. TP=102. Vol=2000. TP*Vol=204,000.
    // C3: High=102, Low=100, Close=101. TP=101. Vol=1000. TP*Vol=101,000.
    // Total TP*Vol = 405,000. Total Vol = 4000.
    // VWAP = 405,000 / 4,000 = 101.25.

    let strategy = VWAPStrategy::default();
    let mut candles = VecDeque::new();
    let ts_start = 86400; // Midnight

    // Helper to create exact candle
    fn exact_candle(h: f64, l: f64, c: f64, v: f64, ts: i64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: dec!(100.0), // Irrelevant for VWAP
            high: Decimal::from_f64(h).unwrap(),
            low: Decimal::from_f64(l).unwrap(),
            close: Decimal::from_f64(c).unwrap(),
            volume: Decimal::from_f64(v).unwrap(),
            timestamp: ts,
        }
    }

    candles.push_back(exact_candle(101.0, 99.0, 100.0, 1000.0, ts_start));
    candles.push_back(exact_candle(103.0, 101.0, 102.0, 2000.0, ts_start + 60));
    candles.push_back(exact_candle(102.0, 100.0, 101.0, 1000.0, ts_start + 120));

    // Manually construct context (ContextBuilder is too simple)
    let ctx = AnalysisContext {
        symbol: "TEST".to_string(),
        current_price: dec!(101.0),
        price_f64: 101.0,
        fast_sma: Some(Decimal::ZERO),
        slow_sma: Some(Decimal::ZERO),
        trend_sma: Some(Decimal::ZERO),
        rsi: Some(dec!(50.0)),
        macd_value: Some(Decimal::ZERO),
        macd_signal: Some(Decimal::ZERO),
        macd_histogram: Some(Decimal::ZERO),
        last_macd_histogram: None,
        atr: Some(Decimal::ONE),
        bb_lower: Some(Decimal::ZERO),
        bb_middle: Some(Decimal::ZERO),
        bb_upper: Some(Decimal::ZERO),
        adx: Some(Decimal::ZERO),
        has_position: false,
        position: None,
        timestamp: ts_start + 120,
        timeframe_features: None,
        candles,
        rsi_history: VecDeque::new(),
        ofi_value: Decimal::ZERO,
        cumulative_delta: Decimal::ZERO,
        volume_profile: None,
        ofi_history: VecDeque::new(),
        hurst_exponent: None,
        skewness: None,
        momentum_normalized: None,
        realized_volatility: None,
        feature_set: None,
    };

    let vwap = strategy
        .calculate_vwap(&ctx)
        .expect("VWAP should be calculated");

    // Assert exact match
    assert_eq!(
        vwap,
        dec!(101.25),
        "VWAP precision failed. Expected 101.25, got {}",
        vwap
    );
}

#[test]
fn test_precision_smc_fvg() {
    // Objective: Verify Fair Value Gap size is calculated exactly as (Low3 - High1) or (Low1 - High3)
    // Setup: Bullish FVG
    // C1: High = 100.0
    // C2: Impulsive
    // C3: Low = 105.0
    // Gap = 105.0 - 100.0 = 5.0.

    let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.0));
    let mut candles = VecDeque::new();

    // Padding (need 5 total)
    for _ in 0..10 {
        candles.push_back(Candle {
            symbol: "TEST".to_string(),
            open: dec!(10.0),
            high: dec!(10.0),
            low: dec!(10.0),
            close: dec!(10.0),
            volume: dec!(100.0),
            timestamp: 0,
        });
    }

    fn fvg_candle(o: f64, h: f64, l: f64, c: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64(o).unwrap(),
            high: Decimal::from_f64(h).unwrap(),
            low: Decimal::from_f64(l).unwrap(),
            close: Decimal::from_f64(c).unwrap(),
            volume: dec!(1000.0),
            timestamp: 0,
        }
    }

    candles.push_back(fvg_candle(95.0, 100.0, 90.0, 95.0)); // C1 High 100
    candles.push_back(fvg_candle(100.0, 110.0, 100.0, 108.0)); // C2 Impulsive bullish (open=100, close=108)
    candles.push_back(fvg_candle(108.0, 120.0, 105.0, 115.0)); // C3 Low 105
    // C4 (last candle): must NOT invalidate (Low >= 100) and MUST close in zone (Close <= 105)
    let c4 = fvg_candle(103.0, 120.0, 103.0, 102.0);
    candles.push_back(c4);

    let result = strategy.detect_fvg(&candles);
    assert!(result.is_some(), "FVG should be detected");
    let (_, gap_size, _) = result.unwrap();

    assert_eq!(
        gap_size,
        dec!(5.0),
        "FVG Gap Size precision failed. Expected 5.0, got {}",
        gap_size
    );
}

#[test]
fn test_precision_zscore() {
    // Objective: Verify Z-Score = (Price - Mean) / StdDev
    // Data: [10, 20, 30]. Mean=20.
    // StdDev (Population or Sample? Statrs usually Sample).
    // Sample StdDev:
    // Mean = 20.
    // (10-20)^2 = 100. (20-20)^2 = 0. (30-20)^2 = 100. Sum=200.
    // Variance = 200 / (3-1) = 100.
    // StdDev = 10.
    // New Price = 40. Z = (40 - 20) / 10 = 2.0.

    let strategy = ZScoreMeanReversionStrategy::new(3, dec!(2.0), dec!(0.0));
    let mut candles = VecDeque::new();

    // We need to provide enough history to satisfy `min_data_points` (20).
    // The strategy only uses the last `lookback_period` (3) for the Z-Score calculation.
    // We inject 17 dummy candles followed by the 3 actual data points [10, 20, 30].

    for _ in 0..17 {
        candles.push_back(Candle {
            symbol: "T".to_string(),
            open: dec!(0),
            high: dec!(0),
            low: dec!(0),
            close: dec!(0),
            volume: dec!(0),
            timestamp: 0,
        });
    }
    candles.push_back(Candle {
        symbol: "T".to_string(),
        open: dec!(0),
        high: dec!(0),
        low: dec!(0),
        close: dec!(10.0),
        volume: dec!(0),
        timestamp: 0,
    });
    candles.push_back(Candle {
        symbol: "T".to_string(),
        open: dec!(0),
        high: dec!(0),
        low: dec!(0),
        close: dec!(20.0),
        volume: dec!(0),
        timestamp: 0,
    });
    candles.push_back(Candle {
        symbol: "T".to_string(),
        open: dec!(0),
        high: dec!(0),
        low: dec!(0),
        close: dec!(30.0),
        volume: dec!(0),
        timestamp: 0,
    });

    let ctx = AnalysisContext {
        symbol: "TEST".to_string(),
        current_price: dec!(40.0),
        price_f64: 40.0,
        fast_sma: Some(Decimal::ZERO),
        slow_sma: Some(Decimal::ZERO),
        trend_sma: Some(Decimal::ZERO),
        rsi: Some(dec!(50.0)),
        macd_value: Some(Decimal::ZERO),
        macd_signal: Some(Decimal::ZERO),
        macd_histogram: Some(Decimal::ZERO),
        last_macd_histogram: None,
        atr: Some(Decimal::ONE),
        bb_lower: Some(Decimal::ZERO),
        bb_middle: Some(Decimal::ZERO),
        bb_upper: Some(Decimal::ZERO),
        adx: Some(Decimal::ZERO),
        has_position: false,
        position: None,
        timestamp: 0,
        timeframe_features: None,
        candles,
        rsi_history: VecDeque::new(),
        ofi_value: Decimal::ZERO,
        cumulative_delta: Decimal::ZERO,
        volume_profile: None,
        ofi_history: VecDeque::new(),
        hurst_exponent: None,
        skewness: None,
        momentum_normalized: None,
        realized_volatility: None,
        feature_set: None,
    };

    let (zscore, _, _) = strategy
        .calculate_stats(&ctx)
        .expect("Z-Score should calculate");

    // Z-Score with current price in sample:
    // Prices = [40, 30, 20]. Mean = 30. StdDev(sample) = 10.
    // Z = (40 - 30) / 10 = 1.0 (current_price is now part of the sample)
    assert_eq!(
        zscore,
        dec!(1.0),
        "Z-Score precision failed. Expected 1.0, got {}",
        zscore
    );
}

#[test]
fn test_precision_rsi_alignment() {
    // Objective: Ensure verification uses the accurate RSI from history
    use crate::application::strategies::legacy::momentum::DivergenceType;

    let strategy = MomentumDivergenceStrategy::default(); // Lookback 14
    let mut candles = VecDeque::new();
    let mut rsi_history = VecDeque::new();

    // Setup:
    // - Total history: 20 candles
    // - Lookback: 14 candles (Start index = 6)
    // - First Low at index 10 (First half of lookback window)
    // - Second Low at index 18 (Second half of lookback window)

    // Let's set Low at index 10 (First Low).
    // And Low at index 18 (Second Low).

    for i in 0..20 {
        candles.push_back(Candle {
            symbol: "T".to_string(),
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1000),
            timestamp: i as i64,
        });
        // RSI history: unique value per index to verify alignment
        rsi_history.push_back(Decimal::from(i));
    }

    // Set Lows
    candles[10].low = dec!(90.0); // First Low
    candles[18].low = dec!(80.0); // Second Low < First Low (Bullish Price)

    // Set RSI at specific indices for verification
    rsi_history[10] = dec!(30.0);
    rsi_history[18] = dec!(35.0);

    let ctx = AnalysisContext {
        symbol: "TEST".to_string(),
        current_price: dec!(80.0),
        price_f64: 80.0,
        fast_sma: Some(Decimal::ZERO),
        slow_sma: Some(Decimal::ZERO),
        trend_sma: Some(Decimal::ZERO),
        rsi: Some(dec!(35.0)),
        macd_value: Some(Decimal::ZERO),
        macd_signal: Some(Decimal::ZERO),
        macd_histogram: Some(Decimal::ZERO),
        last_macd_histogram: None,
        atr: Some(Decimal::ONE),
        bb_lower: Some(Decimal::ZERO),
        bb_middle: Some(Decimal::ZERO),
        bb_upper: Some(Decimal::ZERO),
        adx: Some(Decimal::ZERO),
        has_position: false,
        position: None,
        timestamp: 0,
        timeframe_features: None,
        candles,
        rsi_history,
        ofi_value: Decimal::ZERO,
        cumulative_delta: Decimal::ZERO,
        volume_profile: None,
        ofi_history: VecDeque::new(),
        hurst_exponent: None,
        skewness: None,
        momentum_normalized: None,
        realized_volatility: None,
        feature_set: None,
    };

    let div = strategy.find_divergence(&ctx);
    if div.is_none() {
        panic!("Should find divergence - RSI at second low (35) > RSI at first low (30)");
    }

    if let Some(DivergenceType::Bullish {
        price_low1,
        price_low2,
        rsi_now,
    }) = div
    {
        assert_eq!(price_low1, dec!(90.0));
        assert_eq!(price_low2, dec!(80.0));
        assert_eq!(
            rsi_now,
            dec!(35.0),
            "RSI retrieval precision failed. Expected 35.0 (from index 18), got {}",
            rsi_now
        );
    } else {
        panic!("Expected Bullish divergence");
    }
}
