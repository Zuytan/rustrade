use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};

/// Mean Reversion Strategy
///
/// Captures price buffering against Bollinger Bands.
/// - Buy: Price drops below Lower Band AND RSI is oversold (< 30)
/// - Sell: Price returns to Mean (Middle Band) OR RSI is overbought (> 70)
#[derive(Debug, Clone)]
pub struct MeanReversionStrategy {
    #[allow(dead_code)]
    bb_period: usize,
    rsi_exit_threshold: f64,
}

impl MeanReversionStrategy {
    pub fn new(bb_period: usize, rsi_exit_threshold: f64) -> Self {
        Self {
            bb_period,
            rsi_exit_threshold,
        }
    }
}

impl TradingStrategy for MeanReversionStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // Ensure we have valid data (bands are not 0.0)
        if ctx.bb_upper == 0.0 || ctx.bb_lower == 0.0 {
            return None;
        }

        // Buy Condition: Oversold Deep Value
        // Price below lower band AND RSI < 30
        if ctx.price_f64 < ctx.bb_lower && ctx.rsi < 30.0 {
            return Some(Signal::buy(format!(
                "MeanReversion: Price {:.2} < LowerBB {:.2} & RSI {:.2} < 30",
                ctx.price_f64, ctx.bb_lower, ctx.rsi
            )));
        }

        // Sell Condition: Reverted to Mean OR Overbought
        if ctx.has_position {
            // 1. Reverted to Mean (Middle Band)
            if ctx.price_f64 > ctx.bb_middle {
                return Some(Signal::sell(format!(
                    "MeanReversion: Reverted to Mean (Price {:.2} > MiddleBB {:.2})",
                    ctx.price_f64, ctx.bb_middle
                )));
            }

            // 2. RSI Overbought Protection (in case it blasts through mean without closing)
            if ctx.rsi > self.rsi_exit_threshold {
                return Some(Signal::sell(format!(
                    "MeanReversion: RSI Overbought (RSI {:.2} > {:.2})",
                    ctx.rsi, self.rsi_exit_threshold
                )));
            }
        }

        None
    }

    fn name(&self) -> &str {
        "MeanReversion"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::OrderSide;
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;

    fn create_context(
        price: f64,
        rsi: f64,
        lower: f64,
        middle: f64,
        upper: f64,
        has_pos: bool,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0), // Irrelevant for this logic
            price_f64: price,
            fast_sma: 0.0,
            slow_sma: 0.0,
            trend_sma: 0.0,
            rsi,
            macd_value: 0.0,
            macd_signal: 0.0,
            macd_histogram: 0.0,
            last_macd_histogram: None,
            atr: 1.0,
            bb_lower: lower,
            bb_middle: middle,
            bb_upper: upper,
            adx: 0.0,
            has_position: has_pos,
            timestamp: 0,
            candles: VecDeque::new(),
            rsi_history: VecDeque::new(),
            // OFI fields (defaults for tests)
            ofi_value: 0.0,
            cumulative_delta: 0.0,
            volume_profile: None,
            ofi_history: VecDeque::new(),
            hurst_exponent: None,
            skewness: None,
            momentum_normalized: None,
            realized_volatility: None,
            timeframe_features: None,
        }
    }

    #[test]
    fn test_mean_reversion_buy() {
        let strategy = MeanReversionStrategy::new(20, 70.0);
        // Price 95, Lower 96 -> Below Band. RSI 25 -> Oversold. -> BUY
        let ctx = create_context(95.0, 25.0, 96.0, 100.0, 104.0, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        assert!(matches!(signal.unwrap().side, OrderSide::Buy));
    }

    #[test]
    fn test_mean_reversion_no_buy_if_rsi_high() {
        let strategy = MeanReversionStrategy::new(20, 70.0);
        // Price 95, Lower 96 -> Below Band. RSI 40 -> Not Oversold. -> NO BUY
        let ctx = create_context(95.0, 40.0, 96.0, 100.0, 104.0, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none());
    }

    #[test]
    fn test_mean_reversion_sell_on_mean() {
        let strategy = MeanReversionStrategy::new(20, 70.0);
        // Price 101, Middle 100 -> Above Mean. -> SELL
        let ctx = create_context(101.0, 50.0, 96.0, 100.0, 104.0, true);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        assert!(matches!(signal.unwrap().side, OrderSide::Sell));
    }
}
