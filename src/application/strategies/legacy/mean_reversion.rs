use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::Decimal;

/// Mean Reversion Strategy
///
/// Captures price buffering against Bollinger Bands.
/// - Buy: Price drops below Lower Band AND RSI is oversold (< 30)
/// - Sell: Price returns to Mean (Middle Band) OR RSI is overbought (> 70)
#[derive(Debug, Clone)]
pub struct MeanReversionStrategy {
    rsi_exit_threshold: Decimal,
}

impl MeanReversionStrategy {
    pub fn new(_bb_period: usize, rsi_exit_threshold: Decimal) -> Self {
        Self { rsi_exit_threshold }
    }
}

impl TradingStrategy for MeanReversionStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // Buy Condition: Oversold Deep Value
        // Price below lower band AND RSI < 30
        use rust_decimal_macros::dec;
        let bb_lower = ctx.bb_lower?;
        let bb_middle = ctx.bb_middle?;
        // bb_upper isn't strictly needed for buy, but let's be safe if we need it later or for symmetry
        // logic only uses lower and middle and rsi
        let rsi = ctx.rsi?;

        if ctx.current_price < bb_lower && rsi < dec!(30.0) {
            return Some(Signal::buy(format!(
                "MeanReversion: Price {} < LowerBB {} & RSI {} < 30",
                ctx.current_price, bb_lower, rsi
            )));
        }

        // Sell Condition: Reverted to Mean OR Overbought
        if ctx.has_position {
            // 1. Reverted to Mean (Middle Band)
            if ctx.current_price > bb_middle {
                return Some(Signal::sell(format!(
                    "MeanReversion: Reverted to Mean (Price {} > MiddleBB {})",
                    ctx.current_price, bb_middle
                )));
            }

            // 2. RSI Overbought Protection (in case it blasts through mean without closing)
            if rsi > self.rsi_exit_threshold {
                return Some(Signal::sell(format!(
                    "MeanReversion: RSI Overbought (RSI {} > {})",
                    rsi, self.rsi_exit_threshold
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
    use rust_decimal::prelude::FromPrimitive;
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
            current_price: Decimal::from_f64(price).unwrap(),
            price_f64: price,
            fast_sma: Some(Decimal::ZERO),
            slow_sma: Some(Decimal::ZERO),
            trend_sma: Some(Decimal::ZERO),
            rsi: Some(Decimal::from_f64(rsi).unwrap()),
            macd_value: Some(Decimal::ZERO),
            macd_signal: Some(Decimal::ZERO),
            macd_histogram: Some(Decimal::ZERO),
            last_macd_histogram: None,
            atr: Some(Decimal::ONE),
            bb_lower: Some(Decimal::from_f64(lower).unwrap()),
            bb_middle: Some(Decimal::from_f64(middle).unwrap()),
            bb_upper: Some(Decimal::from_f64(upper).unwrap()),
            adx: Some(Decimal::ZERO),
            has_position: has_pos,
            position: None,
            timestamp: 0,
            candles: VecDeque::new(),
            rsi_history: VecDeque::new(),
            // OFI fields (defaults for tests)
            ofi_value: Decimal::ZERO,
            cumulative_delta: Decimal::ZERO,
            volume_profile: None,
            ofi_history: VecDeque::new(),
            hurst_exponent: None,
            skewness: None,
            momentum_normalized: None,
            realized_volatility: None,
            timeframe_features: None,
            feature_set: None,
        }
    }

    #[test]
    fn test_mean_reversion_buy() {
        let strategy = MeanReversionStrategy::new(20, dec!(70.0));
        // Price 95, Lower 96 -> Below Band. RSI 25 -> Oversold. -> BUY
        let ctx = create_context(95.0, 25.0, 96.0, 100.0, 104.0, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        assert!(matches!(signal.unwrap().side, OrderSide::Buy));
    }

    #[test]
    fn test_mean_reversion_no_buy_if_rsi_high() {
        let strategy = MeanReversionStrategy::new(20, dec!(70.0));
        // Price 95, Lower 96 -> Below Band. RSI 40 -> Not Oversold. -> NO BUY
        let ctx = create_context(95.0, 40.0, 96.0, 100.0, 104.0, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none());
    }

    #[test]
    fn test_mean_reversion_sell_on_mean() {
        let strategy = MeanReversionStrategy::new(20, dec!(70.0));
        // Price 101, Middle 100 -> Above Mean. -> SELL
        let ctx = create_context(101.0, 50.0, 96.0, 100.0, 104.0, true);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        assert!(matches!(signal.unwrap().side, OrderSide::Sell));
    }
}
