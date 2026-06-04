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
                strict_sell_htf_confirmation: false,
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

fn get_all_strategies() -> Vec<Box<dyn TradingStrategy>> {
    let strategies: Vec<Box<dyn TradingStrategy>> = vec![
        Box::new(OrderFlowStrategy::default()),
        Box::new(StatisticalMomentumStrategy::default()),
        Box::new(ZScoreMeanReversionStrategy::default()),
        Box::new(SMCStrategy::default()),
        Box::new(EnsembleStrategy::modern_ensemble(&AnalystConfig::default())),
    ];

    strategies
}

#[test]
fn test_qa_scenario_bull_market() {
    let ctx = ContextBuilder::new(105.0)
        .with_sma(103.0, 100.0, 95.0)
        .with_rsi(65.0)
        .with_adx(35.0)
        .with_macd(0.5)
        .with_candles(100, 100.0)
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
    }
}

#[test]
fn test_qa_scenario_bear_market() {
    let ctx = ContextBuilder::new(95.0)
        .with_sma(97.0, 100.0, 105.0)
        .with_rsi(35.0)
        .with_adx(35.0)
        .with_macd(-0.5)
        .with_candles(100, 100.0)
        .with_position(true)
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
fn test_qa_scenario_insufficient_data() {
    let mut ctx = ContextBuilder::new(100.0).build();
    ctx.candles.clear();
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
    }
}

#[test]
fn test_precision_smc_fvg() {
    let strategy = SMCStrategy::new(20, dec!(0.001), dec!(1.0));
    let mut candles = VecDeque::new();

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

    candles.push_back(fvg_candle(95.0, 100.0, 90.0, 95.0));
    candles.push_back(fvg_candle(100.0, 110.0, 100.0, 108.0));
    candles.push_back(fvg_candle(108.0, 120.0, 105.0, 115.0));
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
    let strategy = ZScoreMeanReversionStrategy::new(3, dec!(2.0), dec!(0.0));
    let mut candles = VecDeque::new();

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
        strict_sell_htf_confirmation: false,
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
    assert_eq!(
        zscore,
        dec!(1.0),
        "Z-Score precision failed. Expected 1.0, got {}",
        zscore
    );
}
