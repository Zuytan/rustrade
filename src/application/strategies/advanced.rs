use super::dual_sma::DualSMAStrategy;
use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::types::OrderSide;

/// Advanced Triple Filter Strategy
///
/// Combines SMA crossover with three additional filters:
/// 1. Trend Filter: Price must be above/below trend SMA
/// 2. RSI Filter: RSI must not be overbought (for buys)
/// 3. MACD Filter: MACD histogram must be positive and rising
#[derive(Debug, Clone)]
pub struct AdvancedTripleFilterStrategy {
    sma_strategy: DualSMAStrategy,
    rsi_threshold: f64,
    #[allow(dead_code)]
    trend_sma_period: usize,
}

impl AdvancedTripleFilterStrategy {
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        sma_threshold: f64,
        trend_sma_period: usize,
        rsi_threshold: f64,
    ) -> Self {
        Self {
            sma_strategy: DualSMAStrategy::new(fast_period, slow_period, sma_threshold),
            rsi_threshold,
            trend_sma_period,
        }
    }

    fn trend_filter(&self, ctx: &AnalysisContext, side: OrderSide) -> bool {
        match side {
            OrderSide::Buy => {
                // For buy: price should be above trend SMA (uptrend)
                ctx.price_f64 > ctx.trend_sma
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
        // MACD histogram should be positive and rising for buys
        if let Some(prev_hist) = ctx.last_macd_histogram {
            ctx.macd_histogram > 0.0 && ctx.macd_histogram > prev_hist
        } else {
            // If no previous histogram, just check if current is positive
            ctx.macd_histogram > 0.0
        }
    }
}

impl TradingStrategy for AdvancedTripleFilterStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // First, get base SMA signal
        let sma_signal = self.sma_strategy.analyze(ctx)?;

        // Apply filters based on signal type
        match sma_signal.side {
            OrderSide::Buy => {
                // All filters must pass for buy signals
                if !self.trend_filter(ctx, OrderSide::Buy) {
                    return None;
                }

                if !self.rsi_filter(ctx, OrderSide::Buy) {
                    return None;
                }

                if !self.macd_filter(ctx) {
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
            bb_middle: 0.0,
            bb_upper: 0.0,
            has_position: false,
            timestamp: 0,
        }
    }

    #[test]
    fn test_advanced_buy_all_filters_pass() {
        let strategy = AdvancedTripleFilterStrategy::new(20, 60, 0.001, 200, 75.0);
        let ctx = create_test_context();

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Advanced Buy"));
    }

    #[test]
    fn test_advanced_buy_rejected_rsi_too_high() {
        let strategy = AdvancedTripleFilterStrategy::new(20, 60, 0.001, 200, 75.0);
        let mut ctx = create_test_context();
        ctx.rsi = 80.0; // Overbought

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy when RSI too high");
    }

    #[test]
    fn test_advanced_buy_rejected_below_trend() {
        let strategy = AdvancedTripleFilterStrategy::new(20, 60, 0.001, 200, 75.0);
        let mut ctx = create_test_context();
        ctx.price_f64 = 95.0; // Below trend SMA of 100

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy below trend");
    }

    #[test]
    fn test_advanced_buy_rejected_macd_negative() {
        let strategy = AdvancedTripleFilterStrategy::new(20, 60, 0.001, 200, 75.0);
        let mut ctx = create_test_context();
        ctx.macd_histogram = -0.1; // Negative

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should reject buy with negative MACD");
    }

    #[test]
    fn test_advanced_sell_signal() {
        let strategy = AdvancedTripleFilterStrategy::new(20, 60, 0.001, 200, 75.0);
        let mut ctx = create_test_context();
        ctx.fast_sma = 98.0; // Below slow SMA
        ctx.slow_sma = 100.0;
        ctx.has_position = true;

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
    }
}
