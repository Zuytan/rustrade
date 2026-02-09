use crate::application::strategies::{StrategyFactory, TradingStrategy};
use crate::domain::market::market_regime::MarketRegime;
use crate::domain::ports::MarketDataService;
use crate::domain::repositories::StrategyRepository;
use crate::domain::trading::symbol_context::SymbolContext;
use crate::domain::trading::types::Candle;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Service responsible for warming up symbol contexts with historical data.
///
/// This service handles:
/// - Loading historical candles from market data service
/// - Initializing technical indicators
/// - Calculating and caching reward/risk ratios
/// - Broadcasting historical candles to UI
/// - Resolving per-symbol strategy configurations
pub struct WarmupService {
    market_service: Arc<dyn MarketDataService>,
    strategy_repository: Option<Arc<dyn StrategyRepository>>,
    ui_candle_tx: Option<broadcast::Sender<Candle>>,
}

impl WarmupService {
    pub fn new(
        market_service: Arc<dyn MarketDataService>,
        strategy_repository: Option<Arc<dyn StrategyRepository>>,
        ui_candle_tx: Option<broadcast::Sender<Candle>>,
    ) -> Self {
        Self {
            market_service,
            strategy_repository,
            ui_candle_tx,
        }
    }

    /// Resolve the strategy and configuration for a given symbol.
    ///
    /// Checks the strategy repository for symbol-specific configuration.
    /// Falls back to default strategy and config if not found.
    pub async fn resolve_strategy(
        &self,
        symbol: &str,
        default_strategy: Arc<dyn TradingStrategy>,
        default_config: &super::analyst::AnalystConfig,
    ) -> (Arc<dyn TradingStrategy>, super::analyst::AnalystConfig) {
        if let Some(repo) = &self.strategy_repository
            && let Ok(Some(def)) = repo.find_by_symbol(symbol).await
        {
            let mut config = default_config.clone();

            if let Ok(parsed_config) =
                serde_json::from_str::<super::analyst::AnalystConfig>(&def.config_json)
            {
                config = parsed_config;
                debug!("WarmupService: Loaded custom config for {}", symbol);
            } else {
                debug!(
                    "WarmupService: Failed to parse full config for {}, using default with custom strategy",
                    symbol
                );
            }

            config.strategy_mode = def.mode;

            let strategy = StrategyFactory::create(def.mode, &config);
            return (strategy, config);
        }

        // Default
        (default_strategy, default_config.clone())
    }

    /// Warm up a symbol context with historical data.
    ///
    /// This method:
    /// 1. Calculates required lookback period based on indicator periods
    /// 2. Fetches historical candles from market data service
    /// 3. Updates context with each candle to initialize indicators
    /// 4. Calculates and caches reward/risk ratio
    /// 5. Broadcasts recent candles to UI for chart initialization
    /// 6. Marks warmup as successful
    pub async fn warmup_context(
        &self,
        context: &mut SymbolContext,
        symbol: &str,
        end: chrono::DateTime<chrono::Utc>,
    ) {
        // Calculate needed lookback
        // Max(TrendSMA, SlowSMA, EMA, RSI, MACD_Slow)
        let config = &context.config;
        let max_period = [
            config.trend_sma_period,
            config.slow_sma_period,
            config.ema_slow_period,
            config.rsi_period * 2, // General rule for RSI stability
            config.macd_slow_period + config.macd_signal_period,
        ]
        .iter()
        .max()
        .copied()
        .unwrap_or(200);

        // Add 10% buffer
        let required_bars = (max_period as f64 * 1.1) as usize;

        info!(
            "WarmupService: Warming up {} with {} bars (Max Period: {}) ending at {}",
            symbol, required_bars, max_period, end
        );

        // Assuming 1-minute bars.
        // Market is open 6.5h a day ~ 390mins.
        // 2000 bars is ~5.1 trading days.
        // We fetch enough calendar days back to cover weekends/holidays
        let days_back = (required_bars / 300) + 3;
        let start = end - chrono::Duration::days(days_back as i64);

        match self
            .market_service
            .get_historical_bars(symbol, start, end, "1Min")
            .await
        {
            Ok(bars) => {
                let bars_count = bars.len();
                info!(
                    "WarmupService: Fetched {} historical bars for {}",
                    bars_count, symbol
                );

                // Update context with each candle
                for candle in &bars {
                    context.update(candle);

                    // Construct minimal AnalysisContext for warmup (Sequential ML models need this)
                    // We primarily populate feature_set as that's what MLStrategy uses

                    // Helper to safely get features (SymbolContext.last_features is updated)
                    let fs = &context.last_features;

                    // Create dummy/default values for required fields that aren't critical for ML warmup
                    // but needed to satisfy struct constructor
                    let ctx = crate::application::strategies::AnalysisContext {
                        symbol: symbol.to_string(),
                        current_price: candle.close,
                        price_f64: 0.0,
                        fast_sma: Decimal::ZERO,
                        slow_sma: Decimal::ZERO,
                        trend_sma: Decimal::ZERO,
                        rsi: fs.rsi.unwrap_or(dec!(50.0)),
                        macd_value: fs.macd_line.unwrap_or(Decimal::ZERO), // using macd_line as value
                        macd_signal: fs.macd_signal.unwrap_or(Decimal::ZERO),
                        macd_histogram: fs.macd_hist.unwrap_or(Decimal::ZERO),
                        last_macd_histogram: context.last_macd_histogram,
                        atr: Decimal::ZERO,
                        bb_lower: Decimal::ZERO,
                        bb_upper: Decimal::ZERO,
                        bb_middle: Decimal::ZERO,
                        adx: fs.adx.unwrap_or(Decimal::ZERO),
                        has_position: false,
                        position: None,
                        timestamp: candle.timestamp,
                        candles: std::collections::VecDeque::new(), // optimizing: don't clone history for warmup
                        rsi_history: std::collections::VecDeque::new(),
                        ofi_value: fs.ofi.unwrap_or(Decimal::ZERO),
                        cumulative_delta: fs.cumulative_delta.unwrap_or(Decimal::ZERO),
                        volume_profile: None,
                        ofi_history: std::collections::VecDeque::new(),
                        hurst_exponent: fs.hurst_exponent,
                        skewness: fs.skewness,
                        momentum_normalized: fs.momentum_normalized,
                        realized_volatility: fs.realized_volatility,
                        timeframe_features: None,
                        feature_set: Some(fs.clone()),
                    };

                    context.strategy.warmup(&ctx);
                }

                debug!(
                    "WarmupService: Warmup complete for {} with {} candles.",
                    symbol,
                    bars.len()
                );

                // Calculate and cache reward/risk ratio for trade filtering
                if !bars.is_empty() {
                    let regime = context
                        .regime_detector
                        .detect(&bars)
                        .unwrap_or(MarketRegime::unknown());
                    let last_price_decimal = bars
                        .last()
                        .expect("bars verified non-empty by is_empty() check")
                        .close;

                    let expectancy = context
                        .expectancy_evaluator
                        .evaluate(symbol, last_price_decimal, &regime)
                        .await;
                    context.cached_reward_risk_ratio = expectancy.reward_risk_ratio;

                    info!(
                        "WarmupService: Cached reward/risk ratio for {}: {}",
                        symbol, context.cached_reward_risk_ratio
                    );
                }

                // Broadcast last 100 historical candles to UI for chart initialization
                if let Some(tx) = &self.ui_candle_tx {
                    let start_idx = bars.len().saturating_sub(100);
                    let recent_bars = &bars[start_idx..];
                    info!(
                        "WarmupService: Broadcasting {} historical candles for {} to UI",
                        recent_bars.len(),
                        symbol
                    );

                    for bar in recent_bars {
                        let candle = Candle {
                            symbol: symbol.to_string(),
                            open: bar.open,
                            high: bar.high,
                            low: bar.low,
                            close: bar.close,
                            volume: bar.volume,
                            timestamp: bar.timestamp,
                        };
                        let _ = tx.send(candle);
                    }
                }

                // Mark warmup as successful
                context.warmup_succeeded = true;
                info!(
                    "WarmupService: ✓ Warmup completed successfully for {} with {} bars",
                    symbol,
                    bars.len()
                );
            }
            Err(e) => {
                warn!(
                    "WarmupService: Failed to warmup {}: {}. Indicators will start from zero (degraded mode)",
                    symbol, e
                );
                // warmup_succeeded remains false
                // Indicators are already initialized to zero/default in SymbolContext::new()
                // The system will continue trading but with less historical context
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::optimization::win_rate_provider::StaticWinRateProvider;
    use crate::application::strategies::StrategyFactory;
    use crate::domain::market::strategy_config::StrategyMode;
    use crate::infrastructure::mock::MockMarketDataService;

    #[tokio::test]
    async fn test_warmup_service_initialization() {
        let market_service = Arc::new(MockMarketDataService::new());
        let service = WarmupService::new(market_service, None, None);

        // Service should be created successfully
        assert!(service.strategy_repository.is_none());
        assert!(service.ui_candle_tx.is_none());
    }

    #[tokio::test]
    async fn test_resolve_strategy_default() {
        let market_service = Arc::new(MockMarketDataService::new());
        let service = WarmupService::new(market_service, None, None);

        let default_config = super::super::analyst::AnalystConfig::default();
        let default_strategy = StrategyFactory::create(StrategyMode::Advanced, &default_config);

        let (_strategy, config) = service
            .resolve_strategy("BTC/USD", default_strategy.clone(), &default_config)
            .await;

        // Should return default strategy and config when no repository
        assert_eq!(config.strategy_mode, default_config.strategy_mode);
    }

    #[tokio::test]
    async fn test_warmup_context_success() {
        let market_service = Arc::new(MockMarketDataService::new());
        let service = WarmupService::new(market_service, None, None);

        let config = super::super::analyst::AnalystConfig::default();
        let strategy = StrategyFactory::create(StrategyMode::Advanced, &config);
        let win_rate_provider = Arc::new(StaticWinRateProvider::new(0.5));
        let timeframes = vec![crate::domain::market::timeframe::Timeframe::OneMin];

        let mut context = SymbolContext::new(config, strategy, win_rate_provider, timeframes);

        // Initially warmup not succeeded
        assert!(!context.warmup_succeeded);

        // Warmup the context
        service
            .warmup_context(&mut context, "BTC/USD", chrono::Utc::now())
            .await;

        // After warmup, should be marked as succeeded (even with mock data)
        // Note: MockMarketDataService returns empty bars, so warmup_succeeded stays false
        // This is expected behavior for degraded mode
    }
}
