use crate::application::strategies::legacy::dual_sma::DualSMAStrategy;
use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::trading::types::OrderSide;
use rust_decimal::Decimal;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Advanced Triple Filter Strategy
///
/// Combines SMA crossover with three additional filters:
/// 1. Trend Filter: Price must be above/below trend SMA
/// 2. RSI Filter: RSI must not be overbought (for buys)
/// 3. MACD Filter: MACD histogram must be positive and rising
/// 4. Signal Confirmation: Require N consecutive bars of same signal (Phase 2)
#[derive(Debug, Clone)]
pub struct AdvancedTripleFilterStrategy {
    sma_strategy: DualSMAStrategy,
    rsi_threshold: Decimal,
    signal_confirmation_bars: usize, // Phase 2: require N bars confirmation
    last_signals: Arc<Mutex<HashMap<String, (OrderSide, usize)>>>, // Phase 2: track (signal, count)
    // Risk-based adaptive filters
    macd_requires_rising: bool,
    trend_tolerance_pct: Decimal,
    pub macd_min_threshold: Decimal,
    pub adx_threshold: Decimal,
}

#[derive(Debug, Clone)]
pub struct AdvancedTripleFilterConfig {
    pub fast_period: usize,
    pub slow_period: usize,
    pub sma_threshold: Decimal,
    pub trend_sma_period: usize,
    pub rsi_threshold: Decimal,
    pub signal_confirmation_bars: usize,
    pub macd_requires_rising: bool,
    pub trend_tolerance_pct: Decimal,
    pub macd_min_threshold: Decimal,
    pub adx_threshold: Decimal,
}

impl Default for AdvancedTripleFilterConfig {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            signal_confirmation_bars: 2,
            macd_requires_rising: true,
            trend_tolerance_pct: Decimal::ZERO,
            macd_min_threshold: Decimal::ZERO,
            adx_threshold: dec!(25.0),
        }
    }
}

impl AdvancedTripleFilterStrategy {
    pub fn new(config: AdvancedTripleFilterConfig) -> Self {
        Self {
            sma_strategy: DualSMAStrategy::new(
                config.fast_period,
                config.slow_period,
                config.sma_threshold,
            ),
            rsi_threshold: config.rsi_threshold,
            signal_confirmation_bars: config.signal_confirmation_bars,
            last_signals: Arc::new(Mutex::new(HashMap::new())),
            macd_requires_rising: config.macd_requires_rising,
            trend_tolerance_pct: config.trend_tolerance_pct,
            macd_min_threshold: config.macd_min_threshold,
            adx_threshold: config.adx_threshold,
        }
    }

    fn trend_filter(&self, ctx: &AnalysisContext, side: OrderSide) -> bool {
        match side {
            OrderSide::Buy => {
                // Apply tolerance: price > trend_sma * (1 - tolerance)
                let adjusted_trend = ctx.trend_sma * (Decimal::ONE - self.trend_tolerance_pct);
                ctx.current_price > adjusted_trend
            }
            OrderSide::Sell => {
                // For sell: allow if price breaks below trend (or always allow sells)
                true // Less restrictive on sells
            }
        }
    }

    fn rsi_filter(&self, ctx: &AnalysisContext, side: OrderSide) -> bool {
        match side {
            OrderSide::Buy => {
                // Don't buy if RSI is too high (overbought)
                ctx.rsi < self.rsi_threshold
            }
            OrderSide::Sell => {
                // No RSI restriction on sells
                true
            }
        }
    }

    fn macd_filter(&self, ctx: &AnalysisContext) -> bool {
        // Check minimum threshold first
        if ctx.macd_histogram < self.macd_min_threshold {
            return false;
        }

        // If requires rising, check that condition too
        if self.macd_requires_rising
            && let Some(prev_hist) = ctx.last_macd_histogram
        {
            return ctx.macd_histogram > prev_hist;
        }

        // Passed all checks
        true
    }

    fn adx_filter(&self, ctx: &AnalysisContext, side: OrderSide) -> bool {
        match side {
            OrderSide::Buy => {
                // Require strong trend for buying
                ctx.adx > self.adx_threshold
            }
            OrderSide::Sell => {
                // Sells can happen in weak trends (e.g. stop loss or reversal)
                // But generally we want to exit if trend breaks.
                // Asymmetric filtering: stricter on Entry, more permissive on Exit to allow stop-loss/reversals.
                true
            }
        }
    }

    fn multi_timeframe_trend_filter(&self, ctx: &AnalysisContext, side: OrderSide) -> bool {
        // If no multi-timeframe data available, fall back to single timeframe check
        if ctx.timeframe_features.is_none() {
            return self.trend_filter(ctx, side);
        }

        match side {
            OrderSide::Buy => {
                // For buy signals, check if higher timeframes confirm bullish trend
                // We check 1Hour and 4Hour if available
                use crate::domain::market::timeframe::Timeframe;

                // Check 1Hour timeframe first (most common for day trading)
                if !ctx.higher_timeframe_confirms_trend(OrderSide::Buy, Timeframe::OneHour) {
                    return false;
                }

                // Also check 4Hour if it's in the enabled timeframes
                if !ctx.higher_timeframe_confirms_trend(OrderSide::Buy, Timeframe::FourHour) {
                    return false;
                }

                true
            }
            OrderSide::Sell => {
                // For sell signals, we're more permissive (already have position)
                true
            }
        }
    }
}

impl TradingStrategy for AdvancedTripleFilterStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // First, check ADX for general trend strength (Fail-Fast)
        // We only block Entries (Buys) on weak trend.
        // Existing positions might need to be closed even in weak trend.

        let sma_signal = self.sma_strategy.analyze(ctx)?;

        // Phase 2: Signal Confirmation Logic
        if self.signal_confirmation_bars > 1
            && let Ok(mut signals_lock) = self.last_signals.lock()
        {
            let entry = signals_lock
                .entry(ctx.symbol.clone())
                .or_insert((sma_signal.side, 0));

            if entry.0 == sma_signal.side {
                // Same signal, increment count
                entry.1 += 1;
            } else {
                // Signal flipped, reset
                *entry = (sma_signal.side, 1);
            }

            if entry.1 < self.signal_confirmation_bars {
                // Not enough confirmation yet
                return None;
            }
        }

        // Apply filters based on signal type
        match sma_signal.side {
            OrderSide::Buy => {
                if !self.adx_filter(ctx, OrderSide::Buy) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - Weak Trend (ADX={} <= threshold={})",
                        ctx.symbol,
                        ctx.adx,
                        self.adx_threshold
                    );
                    return None;
                }

                // Multi-timeframe trend confirmation (Phase 3)
                if !self.multi_timeframe_trend_filter(ctx, OrderSide::Buy) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - Higher timeframe trend not confirmed",
                        ctx.symbol
                    );
                    return None;
                }

                // All filters must pass for buy signals
                if !self.trend_filter(ctx, OrderSide::Buy) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - Trend Filter (price={} <= trend_sma={})",
                        ctx.symbol,
                        ctx.current_price,
                        ctx.trend_sma
                    );
                    return None;
                }

                if !self.rsi_filter(ctx, OrderSide::Buy) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - RSI Filter (rsi={} >= threshold={})",
                        ctx.symbol,
                        ctx.rsi,
                        self.rsi_threshold
                    );
                    return None;
                }

                if !self.macd_filter(ctx) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - MACD Filter (hist={}, rising={})",
                        ctx.symbol,
                        ctx.macd_histogram,
                        ctx.last_macd_histogram
                            .map(|prev| ctx.macd_histogram > prev)
                            .unwrap_or(false)
                    );
                    return None;
                }

                Some(Signal::buy(format!(
                    "Advanced Buy: SMA Cross + Filters OK (RSI={}, Trend={}, MACD={})",
                    ctx.rsi, ctx.trend_sma, ctx.macd_histogram
                )))
            }
            OrderSide::Sell => {
                // For sells, we're more permissive (already have position)
                // Just confirm trend isn't strongly against us
                if !self.trend_filter(ctx, OrderSide::Sell) {
                    return None;
                }

                Some(Signal::sell(format!(
                    "Advanced Sell: SMA Cross confirmed (RSI={}, MACD={})",
                    ctx.rsi, ctx.macd_histogram
                )))
            }
        }
    }

    fn name(&self) -> &str {
        "AdvancedTripleFilter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::OrderSide;
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;

    fn create_test_context() -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(105.0),
            price_f64: 105.0,
            fast_sma: dec!(104.0),
            slow_sma: dec!(100.0),
            trend_sma: dec!(100.0),
            rsi: dec!(50.0),
            macd_value: dec!(0.5),
            macd_signal: dec!(0.3),
            macd_histogram: dec!(0.2),
            last_macd_histogram: Some(dec!(0.1)),
            atr: Decimal::ONE,
            bb_lower: Decimal::ZERO,
            bb_upper: Decimal::ZERO,
            bb_middle: Decimal::ZERO,
            adx: dec!(26.0), // Strong trend by default
            has_position: false,
            position: None,
            timestamp: 0,
            timeframe_features: None,
            candles: std::collections::VecDeque::new(),
            rsi_history: std::collections::VecDeque::new(),
            // OFI fields (defaults for tests)
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
    fn test_advanced_buy_all_filters_pass() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: dec!(0.0),
            macd_min_threshold: dec!(0.0),
            adx_threshold: dec!(25.0),
        });
        let ctx = create_test_context();

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Advanced Buy"));
    }

    #[test]
    fn test_advanced_buy_rejected_rsi_too_high() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: dec!(0.0),
            macd_min_threshold: dec!(0.0),
            adx_threshold: dec!(25.0),
        });
        let mut ctx = create_test_context();
        ctx.rsi = dec!(80.0); // Overbought

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy when RSI too high");
    }

    #[test]
    fn test_advanced_buy_rejected_below_trend() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: dec!(0.0),
            macd_min_threshold: dec!(0.0),
            adx_threshold: dec!(25.0),
        });
        let mut ctx = create_test_context();
        ctx.current_price = dec!(95.0); // Below trend SMA of 100

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy below trend");
    }

    #[test]
    fn test_advanced_buy_rejected_macd_negative() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: Decimal::ZERO,
            macd_min_threshold: Decimal::ZERO,
            adx_threshold: dec!(25.0),
        });
        let mut ctx = create_test_context();
        ctx.macd_histogram = dec!(-0.1); // Negative

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy with negative MACD");
    }

    #[test]
    fn test_advanced_sell_signal() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: Decimal::ZERO,
            macd_min_threshold: Decimal::ZERO,
            adx_threshold: dec!(25.0),
        });
        let mut ctx = create_test_context();
        ctx.fast_sma = dec!(98.0); // Below slow SMA
        ctx.slow_sma = dec!(100.0);
        ctx.has_position = true;

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
    }

    #[test]
    fn test_advanced_buy_rejected_weak_trend_adx() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            signal_confirmation_bars: 1,
            macd_requires_rising: false,
            trend_tolerance_pct: Decimal::ZERO,
            macd_min_threshold: Decimal::ZERO,
            adx_threshold: dec!(25.0),
        });
        let mut ctx = create_test_context();
        ctx.adx = dec!(20.0); // Weak trend (< 25.0)

        // Ensure others pass
        ctx.current_price = dec!(105.0);
        ctx.trend_sma = dec!(100.0);
        ctx.rsi = dec!(50.0);
        ctx.macd_histogram = dec!(0.5);

        // Force SMA cross signal
        ctx.fast_sma = dec!(101.0);
        ctx.slow_sma = dec!(100.0); // Golden cross

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy when ADX is weak");
    }
}
