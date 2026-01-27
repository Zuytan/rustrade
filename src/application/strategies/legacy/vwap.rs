use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

/// VWAP (Volume Weighted Average Price) Strategy
///
/// VWAP is a key institutional indicator showing the average price weighted by volume.
/// - Buy: Price below VWAP - deviation AND RSI < oversold threshold
/// - Sell: Price above VWAP + deviation OR position touches VWAP from below
#[derive(Debug, Clone)]
pub struct VWAPStrategy {
    pub deviation_threshold_pct: f64, // % deviation from VWAP for signal (e.g., 0.02 = 2%)
    pub rsi_oversold: f64,            // RSI threshold for oversold condition
    pub rsi_overbought: f64,          // RSI threshold for overbought condition
}

impl VWAPStrategy {
    pub fn new(deviation_threshold_pct: f64, rsi_oversold: f64, rsi_overbought: f64) -> Self {
        Self {
            deviation_threshold_pct,
            rsi_oversold,
            rsi_overbought,
        }
    }

    /// Calculate VWAP from candle history
    /// VWAP = Σ(Typical Price × Volume) / Σ(Volume)
    /// Typical Price = (High + Low + Close) / 3
    fn calculate_vwap(&self, ctx: &AnalysisContext) -> Option<f64> {
        if ctx.candles.is_empty() {
            return None;
        }

        let mut cumulative_tp_vol = Decimal::ZERO;
        let mut cumulative_vol = Decimal::ZERO;

        for candle in &ctx.candles {
            let volume = candle.volume;

            if volume <= Decimal::ZERO {
                continue;
            }

            let typ_price = (candle.high + candle.low + candle.close) / dec!(3.0);
            cumulative_tp_vol += typ_price * volume;
            cumulative_vol += volume;
        }

        if cumulative_vol > Decimal::ZERO {
            (cumulative_tp_vol / cumulative_vol).to_f64()
        } else {
            None
        }
    }
}

impl Default for VWAPStrategy {
    fn default() -> Self {
        Self {
            deviation_threshold_pct: 0.02, // 2% deviation
            rsi_oversold: 35.0,
            rsi_overbought: 65.0,
        }
    }
}

impl TradingStrategy for VWAPStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let vwap = self.calculate_vwap(ctx)?;

        if vwap <= 0.0 {
            return None;
        }

        let deviation = (ctx.price_f64 - vwap) / vwap;

        // Buy: Price significantly below VWAP AND RSI indicates oversold
        if !ctx.has_position
            && deviation < -self.deviation_threshold_pct
            && ctx.rsi < self.rsi_oversold
        {
            return Some(
                Signal::buy(format!(
                    "VWAP: Price {:.2} is {:.2}% below VWAP {:.2}, RSI {:.1} < {:.0}",
                    ctx.price_f64,
                    deviation * 100.0,
                    vwap,
                    ctx.rsi,
                    self.rsi_oversold
                ))
                .with_confidence(0.80),
            );
        }

        // Sell conditions (only if we have a position)
        if ctx.has_position {
            // 1. Price significantly above VWAP
            if deviation > self.deviation_threshold_pct {
                return Some(
                    Signal::sell(format!(
                        "VWAP: Price {:.2} is {:.2}% above VWAP {:.2} - Taking profit",
                        ctx.price_f64,
                        deviation * 100.0,
                        vwap
                    ))
                    .with_confidence(0.75),
                );
            }

            // 2. RSI overbought
            if ctx.rsi > self.rsi_overbought {
                return Some(
                    Signal::sell(format!(
                        "VWAP: RSI {:.1} > {:.0} (overbought) near VWAP {:.2}",
                        ctx.rsi, self.rsi_overbought, vwap
                    ))
                    .with_confidence(0.70),
                );
            }
        }

        None
    }

    fn name(&self) -> &str {
        "VWAP"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::Candle;
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromPrimitive;
    use std::collections::VecDeque;

    fn mock_candle(high: f64, low: f64, close: f64, volume: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64((high + low) / 2.0).unwrap(),
            high: Decimal::from_f64(high).unwrap(),
            low: Decimal::from_f64(low).unwrap(),
            close: Decimal::from_f64(close).unwrap(),
            volume: Decimal::from_f64(volume).unwrap_or(Decimal::ZERO),
            timestamp: 0,
        }
    }

    fn create_context(
        price: f64,
        rsi: f64,
        candles: VecDeque<Candle>,
        has_position: bool,
    ) -> AnalysisContext {
        use rust_decimal_macros::dec;
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
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
            bb_lower: 0.0,
            bb_middle: 0.0,
            bb_upper: 0.0,
            adx: 25.0,
            has_position,
            timestamp: 0,
            timeframe_features: None,
            candles,
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
            feature_set: None,
        }
    }

    #[test]
    fn test_vwap_calculation() {
        let strategy = VWAPStrategy::default();

        // Create candles with known VWAP
        // Candle 1: TP = (110+90+100)/3 = 100, Vol = 1000 -> TP*Vol = 100000
        // Candle 2: TP = (115+95+105)/3 = 105, Vol = 2000 -> TP*Vol = 210000
        // VWAP = (100000 + 210000) / 3000 = 103.33
        let mut candles = VecDeque::new();
        candles.push_back(mock_candle(110.0, 90.0, 100.0, 1000.0));
        candles.push_back(mock_candle(115.0, 95.0, 105.0, 2000.0));

        let ctx = create_context(100.0, 50.0, candles, false);
        let vwap = strategy.calculate_vwap(&ctx);

        assert!(vwap.is_some());
        let vwap_val = vwap.unwrap();
        assert!(
            (vwap_val - 103.33).abs() < 0.1,
            "VWAP should be ~103.33, got {}",
            vwap_val
        );
    }

    #[test]
    fn test_buy_signal_below_vwap() {
        let strategy = VWAPStrategy::new(0.02, 35.0, 65.0);

        let mut candles = VecDeque::new();
        // VWAP will be around 100
        candles.push_back(mock_candle(105.0, 95.0, 100.0, 1000.0));
        candles.push_back(mock_candle(105.0, 95.0, 100.0, 1000.0));

        // Price 97 = 3% below VWAP (100), RSI 30 < 35 (oversold)
        let ctx = create_context(97.0, 30.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(
            sig.side,
            crate::domain::trading::types::OrderSide::Buy
        ));
        assert!(sig.reason.contains("VWAP"));
    }

    #[test]
    fn test_sell_signal_above_vwap() {
        let strategy = VWAPStrategy::new(0.02, 35.0, 65.0);

        let mut candles = VecDeque::new();
        candles.push_back(mock_candle(105.0, 95.0, 100.0, 1000.0));
        candles.push_back(mock_candle(105.0, 95.0, 100.0, 1000.0));

        // Price 103 = 3% above VWAP (100), has position
        let ctx = create_context(103.0, 50.0, candles, true);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(
            sig.side,
            crate::domain::trading::types::OrderSide::Sell
        ));
    }

    #[test]
    fn test_no_signal_at_vwap() {
        let strategy = VWAPStrategy::new(0.02, 35.0, 65.0);

        let mut candles = VecDeque::new();
        candles.push_back(mock_candle(105.0, 95.0, 100.0, 1000.0));

        // Price exactly at VWAP, RSI neutral
        let ctx = create_context(100.0, 50.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none());
    }
}
