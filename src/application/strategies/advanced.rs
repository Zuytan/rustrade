use super::dual_sma::DualSMAStrategy;
use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::trading::types::OrderSide;
use std::collections::HashMap;

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
    rsi_threshold: f64,
    #[allow(dead_code)]
    trend_sma_period: usize,
    _signal_confirmation_bars: usize, // Phase 2: require N bars confirmation
    _last_signals: HashMap<String, (OrderSide, usize)>, // Phase 2: track (signal, count)
    // Risk-based adaptive filters
    macd_requires_rising: bool,
    trend_tolerance_pct: f64,
    pub macd_min_threshold: f64,
    pub adx_threshold: f64,
}

#[derive(Debug, Clone)]
pub struct AdvancedTripleFilterConfig {
    pub fast_period: usize,
    pub slow_period: usize,
    pub sma_threshold: f64,
    pub trend_sma_period: usize,
    pub rsi_threshold: f64,
    pub signal_confirmation_bars: usize,
    pub macd_requires_rising: bool,
    pub trend_tolerance_pct: f64,
    pub macd_min_threshold: f64,
    pub adx_threshold: f64,
}

impl Default for AdvancedTripleFilterConfig {
    fn default() -> Self {
        Self {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: 0.001,
            trend_sma_period: 200,
            rsi_threshold: 75.0,
            signal_confirmation_bars: 2,
            macd_requires_rising: true,
            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            adx_threshold: 25.0,
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
            trend_sma_period: config.trend_sma_period,
            _signal_confirmation_bars: config.signal_confirmation_bars,
            _last_signals: HashMap::new(),
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
                let adjusted_trend = ctx.trend_sma * (1.0 - self.trend_tolerance_pct);
                ctx.price_f64 > adjusted_trend
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
        if self.macd_requires_rising {
            if let Some(prev_hist) = ctx.last_macd_histogram {
                return ctx.macd_histogram > prev_hist;
            }
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
                 // For now, let's keep it asymmetric like other filters: stricter on Entry.
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

        // Apply filters based on signal type
        match sma_signal.side {
            OrderSide::Buy => {
                 if !self.adx_filter(ctx, OrderSide::Buy) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - Weak Trend (ADX={:.2} <= threshold={:.2})",
                        ctx.symbol, ctx.adx, self.adx_threshold
                    );
                    return None;
                }

                // All filters must pass for buy signals
                if !self.trend_filter(ctx, OrderSide::Buy) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - Trend Filter (price={:.2} <= trend_sma={:.2})",
                        ctx.symbol, ctx.price_f64, ctx.trend_sma
                    );
                    return None;
                }

                if !self.rsi_filter(ctx, OrderSide::Buy) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - RSI Filter (rsi={:.2} >= threshold={:.2})",
                        ctx.symbol, ctx.rsi, self.rsi_threshold
                    );
                    return None;
                }

                if !self.macd_filter(ctx) {
                    tracing::info!(
                        "AdvancedFilter [{}]: BUY BLOCKED - MACD Filter (hist={:.4}, rising={})",
                        ctx.symbol,
                        ctx.macd_histogram,
                        ctx.last_macd_histogram
                            .map(|prev| ctx.macd_histogram > prev)
                            .unwrap_or(false)
                    );
                    return None;
                }

                Some(Signal::buy(format!(
                    "Advanced Buy: SMA Cross + Filters OK (RSI={:.1}, Trend={:.2}, MACD={:.4})",
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
                    "Advanced Sell: SMA Cross confirmed (RSI={:.1}, MACD={:.4})",
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
    use rust_decimal_macros::dec;

    fn create_test_context() -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(105.0),
            price_f64: 105.0,
            fast_sma: 104.0,
            slow_sma: 100.0,
            trend_sma: 100.0,
            rsi: 50.0,
            macd_value: 0.5,
            macd_signal: 0.3,
            macd_histogram: 0.2,
            last_macd_histogram: Some(0.1),
            atr: 1.0,
            bb_lower: 0.0,
            bb_upper: 0.0,
            bb_middle: 0.0,
            adx: 26.0, // Strong trend by default
            has_position: false,
            timestamp: 0,
        }
    }

    #[test]
    fn test_advanced_buy_all_filters_pass() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: 0.001,
            trend_sma_period: 200,
            rsi_threshold: 75.0,
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            adx_threshold: 25.0,
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
            sma_threshold: 0.001,
            trend_sma_period: 200,
            rsi_threshold: 75.0,
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            adx_threshold: 25.0,
        });
        let mut ctx = create_test_context();
        ctx.rsi = 80.0; // Overbought

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy when RSI too high");
    }

    #[test]
    fn test_advanced_buy_rejected_below_trend() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: 0.001,
            trend_sma_period: 200,
            rsi_threshold: 75.0,
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            adx_threshold: 25.0,
        });
        let mut ctx = create_test_context();
        ctx.price_f64 = 95.0; // Below trend SMA of 100

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy below trend");
    }

    #[test]
    fn test_advanced_buy_rejected_macd_negative() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: 0.001,
            trend_sma_period: 200,
            rsi_threshold: 75.0,
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            adx_threshold: 25.0,
        });
        let mut ctx = create_test_context();
        ctx.macd_histogram = -0.1; // Negative

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy with negative MACD");
    }

    #[test]
    fn test_advanced_sell_signal() {
        let strategy = AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: 0.001,
            trend_sma_period: 200,
            rsi_threshold: 75.0,
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            adx_threshold: 25.0,
        });
        let mut ctx = create_test_context();
        ctx.fast_sma = 98.0; // Below slow SMA
        ctx.slow_sma = 100.0;
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
            sma_threshold: 0.001,
            trend_sma_period: 200,
            rsi_threshold: 75.0,
            signal_confirmation_bars: 1,
            macd_requires_rising: false,
            trend_tolerance_pct: 0.0,
            macd_min_threshold: 0.0,
            adx_threshold: 25.0,
        });
        let mut ctx = create_test_context();
        ctx.adx = 20.0; // Weak trend (< 25.0)

        // Ensure others pass
        ctx.price_f64 = 105.0;
        ctx.trend_sma = 100.0;
        ctx.rsi = 50.0;
        ctx.macd_histogram = 0.5;

        // Force SMA cross signal
        ctx.fast_sma = 101.0;
        ctx.slow_sma = 100.0; // Golden cross

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy when ADX is weak");
    }
}
