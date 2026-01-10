use crate::application::market_data::signal_generator::SignalGenerator;
use crate::application::monitoring::feature_engineering_service::TechnicalFeatureEngineeringService;
use crate::application::optimization::expectancy_evaluator::MarketExpectancyEvaluator;
use crate::application::optimization::win_rate_provider::WinRateProvider;
use crate::application::risk_management::position_manager::PositionManager;
use crate::application::strategies::TradingStrategy;
use crate::domain::market::market_regime::MarketRegimeDetector;
use crate::domain::ports::{ExpectancyEvaluator, FeatureEngineeringService};
use crate::domain::trading::types::{Candle, FeatureSet};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Per-symbol trading context managing state, indicators, and strategy.
///
/// This is a domain entity representing all the state needed to analyze
/// and generate trading signals for a single symbol. It encapsulates:
/// - Technical indicators and features
/// - Position management
/// - Strategy execution
/// - Market regime detection
/// - Multi-timeframe analysis
pub struct SymbolContext {
    pub feature_service: Box<dyn FeatureEngineeringService>,
    pub signal_generator: SignalGenerator,
    pub position_manager: PositionManager,
    pub strategy: Arc<dyn TradingStrategy>,
    pub config: crate::application::agents::analyst::AnalystConfig,
    pub last_features: FeatureSet,
    pub regime_detector: MarketRegimeDetector,
    pub expectancy_evaluator: Box<dyn ExpectancyEvaluator>,
    pub taken_profit: bool,
    pub last_entry_time: Option<i64>,
    pub min_hold_time_ms: i64,
    pub active_strategy_mode: crate::domain::market::strategy_config::StrategyMode,
    pub last_macd_histogram: Option<f64>,
    pub cached_reward_risk_ratio: f64,
    pub warmup_succeeded: bool,
    pub candle_history: VecDeque<Candle>,
    // Multi-timeframe support
    pub timeframe_aggregator:
        crate::application::market_data::timeframe_aggregator::TimeframeAggregator,
    pub timeframe_features: HashMap<crate::domain::market::timeframe::Timeframe, FeatureSet>,
    pub enabled_timeframes: Vec<crate::domain::market::timeframe::Timeframe>,
}

impl SymbolContext {
    /// Create a new symbol context with the given configuration and strategy.
    ///
    /// # Arguments
    /// * `config` - Trading configuration for this symbol
    /// * `strategy` - Trading strategy to use
    /// * `win_rate_provider` - Provider for historical win rate data
    /// * `enabled_timeframes` - List of timeframes to track for multi-timeframe analysis
    pub fn new(
        config: crate::application::agents::analyst::AnalystConfig,
        strategy: Arc<dyn TradingStrategy>,
        win_rate_provider: Arc<dyn WinRateProvider>,
        enabled_timeframes: Vec<crate::domain::market::timeframe::Timeframe>,
    ) -> Self {
        let min_hold_time_ms = config.min_hold_time_minutes * 60 * 1000;

        Self {
            feature_service: Box::new(TechnicalFeatureEngineeringService::new(&config)),
            signal_generator: SignalGenerator::new(),
            position_manager: PositionManager::new(),
            strategy,
            config: config.clone(),
            last_features: FeatureSet::default(),
            regime_detector: MarketRegimeDetector::new(20, 25.0, 2.0),
            expectancy_evaluator: Box::new(MarketExpectancyEvaluator::new(1.5, win_rate_provider)),
            taken_profit: false,
            last_entry_time: None,
            min_hold_time_ms,
            active_strategy_mode: config.strategy_mode,
            last_macd_histogram: None,
            cached_reward_risk_ratio: 2.0, // Default to 2:1
            warmup_succeeded: false,
            candle_history: VecDeque::with_capacity(100),
            timeframe_aggregator:
                crate::application::market_data::timeframe_aggregator::TimeframeAggregator::new(),
            timeframe_features: HashMap::new(),
            enabled_timeframes,
        }
    }

    /// Update the context with a new candle.
    ///
    /// This updates:
    /// - Candle history (maintains last 100 candles)
    /// - MACD histogram tracking
    /// - Technical features via feature service
    pub fn update(&mut self, candle: &Candle) {
        // Update candle history (maintain 100-candle limit)
        if self.candle_history.len() >= 100 {
            self.candle_history.pop_front();
        }
        self.candle_history.push_back(candle.clone());

        // Store previous MACD histogram before updating features
        self.last_macd_histogram = self.last_features.macd_hist;
        self.last_features = self.feature_service.update(candle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::StrategyFactory;
    use crate::domain::market::strategy_config::StrategyMode;
    use rust_decimal::Decimal;

    fn create_test_config() -> crate::application::agents::analyst::AnalystConfig {
        crate::application::agents::analyst::AnalystConfig::default()
    }

    fn create_test_candle(symbol: &str, price: f64, timestamp: i64) -> Candle {
        Candle {
            symbol: symbol.to_string(),
            open: Decimal::from_f64_retain(price).unwrap(),
            high: Decimal::from_f64_retain(price * 1.01).unwrap(),
            low: Decimal::from_f64_retain(price * 0.99).unwrap(),
            close: Decimal::from_f64_retain(price).unwrap(),
            volume: 1000.0,
            timestamp,
        }
    }

    #[test]
    fn test_symbol_context_initialization() {
        let config = create_test_config();
        let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        let timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        let context = SymbolContext::new(config.clone(), strategy, win_rate_provider, timeframes);

        assert_eq!(context.candle_history.len(), 0);
        assert!(!context.warmup_succeeded);
        assert!(!context.taken_profit);
        assert_eq!(context.cached_reward_risk_ratio, 2.0);
        assert_eq!(
            context.min_hold_time_ms,
            config.min_hold_time_minutes * 60 * 1000
        );
    }

    #[test]
    fn test_candle_history_management() {
        let config = create_test_config();
        let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        let timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        let mut context = SymbolContext::new(config, strategy, win_rate_provider, timeframes);

        // Add 150 candles (exceeds 100 limit)
        for i in 0..150 {
            let candle = create_test_candle("BTC/USD", 50000.0 + i as f64, i);
            context.update(&candle);
        }

        // Should maintain exactly 100 candles
        assert_eq!(context.candle_history.len(), 100);

        // Should have the most recent 100 candles (50-149)
        assert_eq!(context.candle_history.front().unwrap().timestamp, 50);
        assert_eq!(context.candle_history.back().unwrap().timestamp, 149);
    }

    #[test]
    fn test_macd_histogram_tracking() {
        let config = create_test_config();
        let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        let timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        let mut context = SymbolContext::new(config, strategy, win_rate_provider, timeframes);

        // Initially no MACD histogram
        assert_eq!(context.last_macd_histogram, None);

        // Update with candles to generate MACD data
        for i in 0..50 {
            let candle = create_test_candle("BTC/USD", 50000.0 + i as f64, i);
            context.update(&candle);
        }

        // After updates, MACD histogram should be tracked
        // (actual value depends on feature service calculation)
        // We just verify the mechanism works
        let _first_macd = context.last_macd_histogram;

        let candle = create_test_candle("BTC/USD", 50100.0, 50);
        context.update(&candle);

        // After another update, previous MACD should be stored
        if context.last_features.macd_hist.is_some() {
            assert!(context.last_macd_histogram.is_some());
        }
    }

    #[test]
    fn test_multi_timeframe_initialization() {
        let config = create_test_config();
        let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        let timeframes = vec![
            crate::domain::market::timeframe::Timeframe::OneMin,
            crate::domain::market::timeframe::Timeframe::FiveMin,
            crate::domain::market::timeframe::Timeframe::FifteenMin,
        ];

        let context = SymbolContext::new(config, strategy, win_rate_provider, timeframes.clone());

        assert_eq!(context.enabled_timeframes.len(), 3);
        assert_eq!(context.timeframe_features.len(), 0); // Empty until populated
        assert_eq!(context.enabled_timeframes, timeframes);
    }
}
// Force recompile
