use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::prelude::*;

/// Statistical Momentum Strategy
///
/// ATR-normalized momentum for comparable signals across different assets and volatility regimes.
/// Unlike traditional momentum, this normalizes by volatility to avoid false signals during
/// high/low volatility periods.
///
/// Formula: Momentum = (Price - Price_N) / ATR
/// - Positive momentum > threshold + trend confirmation = Buy
/// - Negative momentum or trend break = Sell
///
/// Advantages over traditional momentum:
/// - Volatility-adjusted (comparable across assets)
/// - No lag from moving averages
/// - Trend confirmation prevents whipsaws
#[derive(Debug, Clone)]
pub struct StatisticalMomentumStrategy {
    pub lookback_period: usize,
    pub momentum_threshold: Decimal, // Minimum normalized momentum for signal
    pub trend_confirmation: bool,    // Require price > trend SMA
}

impl StatisticalMomentumStrategy {
    pub fn new(
        lookback_period: usize,
        momentum_threshold: Decimal,
        trend_confirmation: bool,
    ) -> Self {
        Self {
            lookback_period,
            momentum_threshold,
            trend_confirmation,
        }
    }

    /// Calculate ATR-normalized momentum
    /// Momentum = (Current Price - Price_N periods ago) / ATR
    fn calculate_normalized_momentum(&self, ctx: &AnalysisContext) -> Option<Decimal> {
        if ctx.candles.len() < self.lookback_period {
            return None;
        }

        if ctx.atr <= Decimal::ZERO {
            return None; // Invalid ATR
        }

        // Get price N periods ago (nth() is 0-indexed: nth(0) = last, nth(N-1) = N ago)
        let past_candle = ctx.candles.iter().rev().nth(self.lookback_period - 1)?;
        let past_price = past_candle.close;

        // Normalized Momentum = (Current - Past) / ATR
        let raw_momentum = ctx.current_price - past_price;
        let normalized_momentum = raw_momentum / ctx.atr;

        Some(normalized_momentum)
    }

    /// Check if trend confirmation is satisfied
    fn check_trend_confirmation(&self, ctx: &AnalysisContext, is_bullish: bool) -> bool {
        if !self.trend_confirmation {
            return true; // No confirmation required
        }

        if is_bullish {
            ctx.current_price > ctx.trend_sma
        } else {
            ctx.current_price < ctx.trend_sma
        }
    }
}

impl Default for StatisticalMomentumStrategy {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self::new(
            10,        // 10-period lookback
            dec!(1.5), // 1.5 ATR minimum momentum
            true,      // Require trend confirmation
        )
    }
}

impl TradingStrategy for StatisticalMomentumStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let momentum = self.calculate_normalized_momentum(ctx)?;

        // BUY: Strong positive momentum + trend confirmation. Confidence scales with momentum strength.
        if !ctx.has_position
            && momentum > self.momentum_threshold
            && self.check_trend_confirmation(ctx, true)
        {
            let th_f64 = self.momentum_threshold.to_f64().unwrap_or(1.5);
            let mom_f64 = momentum.to_f64().unwrap_or(th_f64);
            let excess = (mom_f64 - th_f64) / th_f64.max(0.01);
            let confidence = (0.5 + (excess * 0.3)).min(0.95);
            return Some(
                Signal::buy(format!(
                    "StatMomentum: Strong upward momentum ({} ATR), Price {} > Trend {}",
                    momentum, ctx.current_price, ctx.trend_sma
                ))
                .with_confidence(confidence),
            );
        }

        // SELL: Momentum weakening or trend break
        if ctx.has_position {
            // Exit if momentum turns negative. Confidence scales with reversal magnitude.
            if momentum < -self.momentum_threshold {
                let th_f64 = self.momentum_threshold.to_f64().unwrap_or(1.5);
                let mom_abs = momentum.abs().to_f64().unwrap_or(0.0);
                let strength = (mom_abs / th_f64).min(2.0);
                let confidence = (0.5 + (strength * 0.25)).min(0.90);
                return Some(
                    Signal::sell(format!(
                        "StatMomentum: Momentum reversed ({} ATR)",
                        momentum
                    ))
                    .with_confidence(confidence),
                );
            }

            // Exit if trend breaks (price below trend SMA). Confidence scales with distance below.
            if self.trend_confirmation && ctx.current_price < ctx.trend_sma {
                let distance_pct = if ctx.trend_sma > Decimal::ZERO {
                    ((ctx.trend_sma - ctx.current_price) / ctx.trend_sma)
                        .to_f64()
                        .unwrap_or(0.0)
                        * 100.0
                } else {
                    0.0
                };
                let confidence = (0.5 + (distance_pct * 0.02)).min(0.85);
                return Some(
                    Signal::sell(format!(
                        "StatMomentum: Trend break (Price {} < Trend {})",
                        ctx.current_price, ctx.trend_sma
                    ))
                    .with_confidence(confidence),
                );
            }
        }

        None
    }

    fn name(&self) -> &str {
        "StatMomentum"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::{Candle, OrderSide};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;

    fn mock_candle(close: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(close).unwrap(),
            high: Decimal::from_f64_retain(close).unwrap() * dec!(1.01),
            low: Decimal::from_f64_retain(close).unwrap() * dec!(0.99),
            close: Decimal::from_f64_retain(close).unwrap(),
            volume: dec!(1000.0),
            timestamp: 0,
        }
    }

    fn create_context(
        price: f64,
        trend_sma: f64,
        atr: f64,
        candles: VecDeque<Candle>,
        has_position: bool,
    ) -> AnalysisContext {
        let d_price = Decimal::from_f64_retain(price).unwrap();
        let d_trend_sma = Decimal::from_f64_retain(trend_sma).unwrap();
        let d_atr = Decimal::from_f64_retain(atr).unwrap();

        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: d_price,
            price_f64: price,
            fast_sma: Decimal::ZERO,
            slow_sma: Decimal::ZERO,
            trend_sma: d_trend_sma,
            rsi: dec!(50.0),
            macd_value: Decimal::ZERO,
            macd_signal: Decimal::ZERO,
            macd_histogram: Decimal::ZERO,
            last_macd_histogram: None,
            atr: d_atr,
            bb_lower: Decimal::ZERO,
            bb_middle: Decimal::ZERO,
            bb_upper: Decimal::ZERO,
            adx: dec!(25.0),
            has_position,
            position: None,
            timestamp: 0,
            timeframe_features: None,
            candles,
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
    fn test_buy_signal_strong_momentum() {
        let strategy = StatisticalMomentumStrategy::default();

        // Create candles: price rising from 90 to 105
        let mut candles = VecDeque::new();
        for i in 0..15 {
            candles.push_back(mock_candle(90.0 + i as f64));
        }

        // Current price = 105, 10 periods ago = 95, ATR = 2.0
        // Momentum = (105 - 95) / 2.0 = 5.0 (> 1.5 threshold)
        let ctx = create_context(105.0, 100.0, 2.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("StatMomentum"));
    }

    #[test]
    fn test_no_buy_below_trend() {
        let strategy = StatisticalMomentumStrategy::default();

        let mut candles = VecDeque::new();
        for i in 0..15 {
            candles.push_back(mock_candle(90.0 + i as f64));
        }

        // Strong momentum but price < trend_sma (105 < 110)
        let ctx = create_context(105.0, 110.0, 2.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none(), "Should not buy below trend");
    }

    #[test]
    fn test_sell_on_momentum_reversal() {
        let strategy = StatisticalMomentumStrategy::default();

        // Create candles: price falling from 105 to 90
        let mut candles = VecDeque::new();
        for i in 0..15 {
            candles.push_back(mock_candle(105.0 - i as f64));
        }

        // Current price = 90, 10 periods ago = 100, ATR = 2.0
        // Momentum = (90 - 100) / 2.0 = -5.0 (< -1.5 threshold)
        let ctx = create_context(90.0, 95.0, 2.0, candles, true);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert!(sig.reason.contains("reversed"));
    }

    #[test]
    fn test_sell_on_trend_break() {
        let strategy = StatisticalMomentumStrategy::default();

        let mut candles = VecDeque::new();
        for _i in 0..15 {
            candles.push_back(mock_candle(100.0));
        }

        // Weak momentum but price broke below trend
        let ctx = create_context(95.0, 100.0, 2.0, candles, true);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert!(sig.reason.contains("Momentum") || sig.reason.contains("Trend"));
    }

    #[test]
    fn test_no_signal_insufficient_data() {
        let strategy = StatisticalMomentumStrategy::default();

        // Only 5 candles (< 10 lookback)
        let mut candles = VecDeque::new();
        for _ in 0..5 {
            candles.push_back(mock_candle(100.0));
        }

        let ctx = create_context(105.0, 100.0, 2.0, candles, false);
        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none());
    }
}
