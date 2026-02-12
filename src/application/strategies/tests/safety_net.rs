use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::strategies::{AnalysisContext, StrategyFactory, TradingStrategy};
use crate::domain::market::strategy_config::StrategyMode;
use rust_decimal::Decimal;
use std::collections::VecDeque;

fn create_empty_context() -> AnalysisContext {
    AnalysisContext {
        symbol: "TEST".to_string(),
        current_price: Decimal::ZERO,
        price_f64: 0.0,
        fast_sma: None,
        slow_sma: None,
        trend_sma: None,
        rsi: None,
        macd_value: None,
        macd_signal: None,
        macd_histogram: None,
        last_macd_histogram: None,
        atr: None,
        bb_lower: None,
        bb_middle: None,
        bb_upper: None,
        adx: None,
        has_position: false,
        position: None,
        timestamp: 0,
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
    }
}

#[test]
fn test_strategies_handle_cold_start_gracefully() {
    let ctx = create_empty_context();
    let config = AnalystConfig::default();

    // List of strategies to test
    let strategies = vec![
        StrategyMode::StatMomentum,
        StrategyMode::ZScoreMR,
        // StrategyMode::OrderFlow, // Might need OFI data?
        StrategyMode::SMC,
        StrategyMode::Dynamic,
        // StrategyMode::Ensemble, // Needs sub-strategies
    ];

    for mode in strategies {
        let strategy = StrategyFactory::create(mode, &config);
        let signal = strategy.analyze(&ctx);

        assert!(
            signal.is_none(),
            "Strategy {:?} produced a signal with empty context! Signal: {:?}",
            mode,
            signal
        );
    }
}

#[test]
fn test_legacy_strategies_handle_cold_start() {
    let ctx = create_empty_context();

    // Legacy strategies via direct instantiation or factory if supported
    // Factory supports some?
    // Let's manually test legacy ones to be sure
    use crate::application::strategies::legacy::{
        AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy, BreakoutStrategy,
        DualSMAStrategy, MeanReversionStrategy, MomentumDivergenceStrategy, TrendRidingStrategy,
        VWAPStrategy,
    };
    use rust_decimal_macros::dec;

    let strategies: Vec<Box<dyn TradingStrategy>> = vec![
        Box::new(AdvancedTripleFilterStrategy::new(
            AdvancedTripleFilterConfig::default(),
        )),
        Box::new(BreakoutStrategy::new(20, dec!(0.02), dec!(1.5))),
        Box::new(DualSMAStrategy::new(10, 20, dec!(0.01))),
        Box::new(MeanReversionStrategy::new(20, dec!(70.0))),
        Box::new(MomentumDivergenceStrategy::new(10, dec!(0.02))),
        Box::new(TrendRidingStrategy::new(20, 50, dec!(0.01), dec!(0.02))),
        Box::new(VWAPStrategy::new(dec!(0.02), dec!(30.0), dec!(70.0))),
    ];

    for strategy in strategies {
        let signal = strategy.analyze(&ctx);
        assert!(
            signal.is_none(),
            "Legacy Strategy {} produced a signal with empty context!",
            strategy.name()
        );
    }
}
