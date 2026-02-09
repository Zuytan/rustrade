use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// VWAP (Volume Weighted Average Price) Strategy
///
/// VWAP is a key institutional indicator showing the average price weighted by volume.
/// - Buy: Price below VWAP - deviation AND RSI < oversold threshold
/// - Sell: Price above VWAP + deviation OR position touches VWAP from below
#[derive(Debug, Clone)]
pub struct VWAPStrategy {
    pub deviation_threshold_pct: Decimal, // % deviation from VWAP for signal (e.g., 0.02 = 2%)
    pub rsi_oversold: Decimal,            // RSI threshold for oversold condition
    pub rsi_overbought: Decimal,          // RSI threshold for overbought condition
}

impl VWAPStrategy {
    pub fn new(
        deviation_threshold_pct: Decimal,
        rsi_oversold: Decimal,
        rsi_overbought: Decimal,
    ) -> Self {
        Self {
            deviation_threshold_pct,
            rsi_oversold,
            rsi_overbought,
        }
    }

    /// Calculate VWAP from candle history
    /// VWAP = Σ(Typical Price × Volume) / Σ(Volume)
    /// Typical Price = (High + Low + Close) / 3
    pub(crate) fn calculate_vwap(&self, ctx: &AnalysisContext) -> Option<Decimal> {
        if ctx.candles.is_empty() {
            return None;
        }

        // Get current day start (midnight UTC) to reset VWAP
        // timestamp is i64 seconds, so modulo 86400 gives seconds since midnight
        let current_ts = ctx.timestamp;
        let day_start = current_ts - (current_ts % 86400);

        // Check data sufficiency: Do we have data since the start of the day?
        // If the first candle is after day_start, we are missing early volume data.
        // This would make VWAP calculation incorrect (it would be a "Rolling VWAP" of available data).
        if let Some(first_candle) = ctx.candles.front().filter(|c| c.timestamp > day_start) {
            tracing::warn!(
                "VWAP: Insufficient data. First candle ts {} > Day start {}. Cannot calculate accurate Daily VWAP.",
                first_candle.timestamp,
                day_start
            );
            return None;
        }

        let mut cumulative_tp_vol = Decimal::ZERO;
        let mut cumulative_vol = Decimal::ZERO;

        for candle in &ctx.candles {
            // Only include candles from current trading day
            if candle.timestamp < day_start {
                continue;
            }
            let volume = candle.volume;

            if volume <= Decimal::ZERO {
                continue;
            }

            let typ_price = (candle.high + candle.low + candle.close) / dec!(3.0);
            cumulative_tp_vol += typ_price * volume;
            cumulative_vol += volume;
        }

        if cumulative_vol > Decimal::ZERO {
            Some(cumulative_tp_vol / cumulative_vol)
        } else {
            None
        }
    }
}

impl Default for VWAPStrategy {
    fn default() -> Self {
        Self {
            deviation_threshold_pct: dec!(0.02), // 2% deviation
            rsi_oversold: dec!(35.0),
            rsi_overbought: dec!(65.0),
        }
    }
}

impl TradingStrategy for VWAPStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let vwap = self.calculate_vwap(ctx)?;

        if vwap <= Decimal::ZERO {
            return None;
        }

        let deviation = (ctx.current_price - vwap) / vwap;

        // Buy: Price significantly below VWAP AND RSI indicates oversold
        if !ctx.has_position
            && deviation < -self.deviation_threshold_pct
            && ctx.rsi < self.rsi_oversold
        {
            return Some(
                Signal::buy(format!(
                    "VWAP: Price {} is {}% below VWAP {}, RSI {} < {}",
                    ctx.current_price,
                    deviation * dec!(100.0),
                    vwap,
                    ctx.rsi,
                    self.rsi_oversold
                ))
                .with_confidence(0.80),
            );
        }

        // Sell conditions (Exit Long OR Enter Short)
        // 1. Price significantly above VWAP
        if deviation > self.deviation_threshold_pct {
            return Some(
                Signal::sell(format!(
                    "VWAP: Price {} is {}% above VWAP {} (Overextended)",
                    ctx.current_price,
                    deviation * dec!(100.0),
                    vwap
                ))
                .with_confidence(0.75),
            );
        }

        // 2. RSI overbought
        if ctx.rsi > self.rsi_overbought {
            return Some(
                Signal::sell(format!(
                    "VWAP: RSI {} > {} (overbought) near VWAP {}",
                    ctx.rsi, self.rsi_overbought, vwap
                ))
                .with_confidence(0.70),
            );
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

    fn mock_candle_with_ts(high: f64, low: f64, close: f64, volume: f64, ts: i64) -> Candle {
        let d_high = Decimal::from_f64(high).unwrap();
        let d_low = Decimal::from_f64(low).unwrap();
        let d_close = Decimal::from_f64(close).unwrap();
        let d_volume = Decimal::from_f64(volume).unwrap_or(Decimal::ZERO);

        Candle {
            symbol: "TEST".to_string(),
            open: (d_high + d_low) / dec!(2.0),
            high: d_high,
            low: d_low,
            close: d_close,
            volume: d_volume,
            timestamp: ts,
        }
    }

    fn create_context(
        price: f64,
        rsi: f64,
        candles: VecDeque<Candle>,
        has_position: bool,
    ) -> AnalysisContext {
        let d_price = Decimal::from_f64(price).unwrap();
        let d_rsi = Decimal::from_f64(rsi).unwrap();
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: d_price,
            price_f64: price,
            fast_sma: Decimal::ZERO,
            slow_sma: Decimal::ZERO,
            trend_sma: Decimal::ZERO,
            rsi: d_rsi,
            macd_value: Decimal::ZERO,
            macd_signal: Decimal::ZERO,
            macd_histogram: Decimal::ZERO,
            last_macd_histogram: None,
            atr: Decimal::ONE,
            bb_lower: Decimal::ZERO,
            bb_middle: Decimal::ZERO,
            bb_upper: Decimal::ZERO,
            adx: dec!(25.0),
            has_position,
            position: None,
            timestamp: candles.back().map(|c| c.timestamp).unwrap_or(100000),
            timeframe_features: None,
            candles,
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
            feature_set: None,
        }
    }

    #[test]
    fn test_vwap_calculation() {
        let strategy = VWAPStrategy::default();

        // Midnight is 0.
        // Candle 1 at 100 which is > 0.
        // Needs start > day_start?
        // timestamp is 100000. day_start = 100000 - (100000 % 86400) = 86400.
        // So candles must be >= 86400.

        let ts_start = 86400; // Start at midnight

        let mut candles = VecDeque::new();
        candles.push_back(mock_candle_with_ts(110.0, 90.0, 100.0, 1000.0, ts_start));
        candles.push_back(mock_candle_with_ts(
            115.0,
            95.0,
            105.0,
            2000.0,
            ts_start + 60,
        ));

        let ctx = create_context(100.0, 50.0, candles, false);
        // Ensure ctx timestamp updates

        let vwap = strategy.calculate_vwap(&ctx);

        assert!(vwap.is_some());
        let vwap_val = vwap.unwrap();
        assert!(
            (vwap_val - dec!(103.33)).abs() < dec!(0.1),
            "VWAP should be ~103.33, got {}",
            vwap_val
        );
    }

    #[test]
    fn test_buy_signal_below_vwap() {
        let strategy = VWAPStrategy::new(dec!(0.02), dec!(35.0), dec!(65.0));

        let ts_start = 86400;
        let mut candles = VecDeque::new();
        candles.push_back(mock_candle_with_ts(105.0, 95.0, 100.0, 1000.0, ts_start));
        candles.push_back(mock_candle_with_ts(
            105.0,
            95.0,
            100.0,
            1000.0,
            ts_start + 60,
        ));

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
    fn test_sell_signal_short_entry() {
        let strategy = VWAPStrategy::new(dec!(0.02), dec!(35.0), dec!(65.0));

        let ts_start = 86400;
        let mut candles = VecDeque::new();
        candles.push_back(mock_candle_with_ts(105.0, 95.0, 100.0, 1000.0, ts_start));
        candles.push_back(mock_candle_with_ts(
            105.0,
            95.0,
            100.0,
            1000.0,
            ts_start + 60,
        ));

        // Price 103 = 3% above VWAP (100), NO POSITION
        // Should trigger Short Entry
        let ctx = create_context(103.0, 50.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some(), "Should signal Sell for Short Entry");
        let sig = signal.unwrap();
        assert!(matches!(
            sig.side,
            crate::domain::trading::types::OrderSide::Sell
        ));
    }

    #[test]
    fn test_insufficient_data_check() {
        let strategy = VWAPStrategy::default();

        // Current time: 100000 (~1 day + 3.7h). Day start: 86400.
        // First candle time: 90000 (valid, > 86400).
        // BUT if first candle is 86400 + 10 = 86410, it's valid.

        // Scenario: Trading day started at 86400.
        // We connect at 90000. We fetch 100 candles.
        // 100 candles * 60s = 6000s = 1.6h.
        // 90000 - 6000 = 84000.
        // 84000 < 86400.
        // So the first candle IN THE LIST is 84000 (yesterday).
        // calculate_vwap filters out < day_start. So it will use only today's candles.
        // THIS IS FINE. We have data crossing the boundary.

        // Scenario 2: Connecting at 90000. Fetch 10 candles.
        // Start time = 89400.
        // 89400 > 86400.
        // We are missing data from 86400 to 89400.
        // VWAP will be incorrect.
        // The check `first_candle.timestamp > day_start` should catch this.

        let current_ts = 100000;
        let late_start = 90000;

        let mut candles = VecDeque::new();
        candles.push_back(mock_candle_with_ts(100.0, 100.0, 100.0, 1000.0, late_start));

        let mut ctx = create_context(100.0, 50.0, candles, false);
        ctx.timestamp = current_ts; // Ensure context knows current time

        let vwap = strategy.calculate_vwap(&ctx);
        assert!(
            vwap.is_none(),
            "Should fail due to insufficient history (start > day_start)"
        );
    }
}
