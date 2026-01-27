use crate::application::strategies::traits::{AnalysisContext, Signal, TradingStrategy};
use rust_decimal::prelude::*;
use statrs::statistics::{Data, Distribution};

/// Z-Score Mean Reversion Strategy
///
/// Modern statistical approach to mean reversion using Z-Scores instead of Bollinger Bands.
/// - Entry: Z-Score < -2.0 (price significantly below mean)
/// - Exit: Z-Score > 0.0 (price returns to mean or above)
///
/// Advantages over Bollinger Bands:
/// - No lag from SMA calculation
/// - Statistically rigorous (2 std devs = 95% confidence)
/// - Adaptive to volatility changes
#[derive(Debug, Clone)]
pub struct ZScoreMeanReversionStrategy {
    pub lookback_period: usize,
    pub entry_threshold: f64, // Typically -2.0 (2 std devs below mean)
    pub exit_threshold: f64,  // Typically 0.0 (return to mean)
    pub min_data_points: usize,
}

impl ZScoreMeanReversionStrategy {
    pub fn new(lookback_period: usize, entry_threshold: f64, exit_threshold: f64) -> Self {
        Self {
            lookback_period,
            entry_threshold,
            exit_threshold,
            min_data_points: lookback_period.max(20),
        }
    }

    /// Calculate Z-Score: (Price - Mean) / StdDev
    fn calculate_zscore(&self, ctx: &AnalysisContext) -> Option<f64> {
        if ctx.candles.len() < self.min_data_points {
            return None;
        }

        // Extract closing prices for lookback period
        let prices: Vec<f64> = ctx
            .candles
            .iter()
            .rev()
            .take(self.lookback_period)
            .filter_map(|c| c.close.to_f64())
            .collect();

        if prices.len() < self.lookback_period {
            return None;
        }

        // Calculate mean and std dev using statrs
        let data = Data::new(prices);
        let mean = data.mean()?;
        let std_dev = data.std_dev()?;

        if std_dev == 0.0 {
            return None; // Avoid division by zero
        }

        // Z-Score = (Current Price - Mean) / StdDev
        let zscore = (ctx.price_f64 - mean) / std_dev;
        Some(zscore)
    }
}

impl Default for ZScoreMeanReversionStrategy {
    fn default() -> Self {
        Self::new(
            20,   // 20-period lookback
            -2.0, // Entry at 2 std devs below mean
            0.0,  // Exit at mean
        )
    }
}

impl TradingStrategy for ZScoreMeanReversionStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let zscore = self.calculate_zscore(ctx)?;

        // BUY: Price significantly below mean (oversold)
        if !ctx.has_position && zscore < self.entry_threshold {
            return Some(
                Signal::buy(format!(
                    "Z-Score MR: Price {:.2} is {:.2} std devs below mean (Z={:.2})",
                    ctx.price_f64,
                    zscore.abs(),
                    zscore
                ))
                .with_confidence(0.85), // High confidence for statistical signals
            );
        }

        // SELL: Price returned to mean or above
        if ctx.has_position && zscore > self.exit_threshold {
            return Some(
                Signal::sell(format!(
                    "Z-Score MR: Price {:.2} returned to mean (Z={:.2})",
                    ctx.price_f64, zscore
                ))
                .with_confidence(0.80),
            );
        }

        None
    }

    fn name(&self) -> &str {
        "ZScoreMR"
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
            open: Decimal::from_f64(close).unwrap(),
            high: Decimal::from_f64(close * 1.01).unwrap(),
            low: Decimal::from_f64(close * 0.99).unwrap(),
            close: Decimal::from_f64(close).unwrap(),
            volume: dec!(1000.0),
            timestamp: 0,
        }
    }

    fn create_context(
        price: f64,
        candles: VecDeque<Candle>,
        has_position: bool,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: Decimal::from_f64(price).unwrap(),
            price_f64: price,
            fast_sma: 0.0,
            slow_sma: 0.0,
            trend_sma: 0.0,
            rsi: 50.0,
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
    fn test_zscore_buy_signal() {
        let strategy = ZScoreMeanReversionStrategy::default();

        // Create candles with mean ~100, some variance
        let mut candles = VecDeque::new();
        let prices = vec![
            98.0, 99.0, 100.0, 101.0, 102.0, 100.0, 99.0, 101.0, 100.0, 98.0, 99.0, 100.0, 101.0,
            100.0, 99.0, 100.0, 101.0, 100.0, 99.0, 100.0, 101.0, 100.0, 99.0, 100.0, 101.0,
        ];
        for price in prices {
            candles.push_back(mock_candle(price));
        }

        // Current price = 90 (significantly below mean ~100)
        // This should trigger Z < -2.0
        let ctx = create_context(90.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(
            signal.is_some(),
            "Should generate buy signal when price significantly below mean"
        );
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Z-Score MR"));
    }

    #[test]
    fn test_zscore_sell_signal() {
        let strategy = ZScoreMeanReversionStrategy::default();

        // Create candles with mean ~100, some variance
        let mut candles = VecDeque::new();
        let prices = vec![
            98.0, 99.0, 100.0, 101.0, 102.0, 100.0, 99.0, 101.0, 100.0, 98.0, 99.0, 100.0, 101.0,
            100.0, 99.0, 100.0, 101.0, 100.0, 99.0, 100.0, 101.0, 100.0, 99.0, 100.0, 101.0,
        ];
        for price in prices {
            candles.push_back(mock_candle(price));
        }

        // Current price = 100 (at mean), has position
        let ctx = create_context(100.0, candles, true);

        let signal = strategy.analyze(&ctx);
        assert!(
            signal.is_some(),
            "Should generate sell signal when price returns to mean"
        );
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
    }

    #[test]
    fn test_no_signal_insufficient_data() {
        let strategy = ZScoreMeanReversionStrategy::default();

        // Only 5 candles (insufficient)
        let mut candles = VecDeque::new();
        for _ in 0..5 {
            candles.push_back(mock_candle(100.0));
        }

        let ctx = create_context(90.0, candles, false);
        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none());
    }

    #[test]
    fn test_no_signal_within_threshold() {
        let strategy = ZScoreMeanReversionStrategy::default();

        // Create candles with mean ~100, some variance
        let mut candles = VecDeque::new();
        let prices = vec![
            98.0, 99.0, 100.0, 101.0, 102.0, 100.0, 99.0, 101.0, 100.0, 98.0, 99.0, 100.0, 101.0,
            100.0, 99.0, 100.0, 101.0, 100.0, 99.0, 100.0, 101.0, 100.0, 99.0, 100.0, 101.0,
        ];
        for price in prices {
            candles.push_back(mock_candle(price));
        }

        // Current price = 99 (only slightly below mean, Z > -2.0)
        let ctx = create_context(99.0, candles, false);

        let signal = strategy.analyze(&ctx);
        assert!(signal.is_none(), "Should not signal for minor deviations");
    }
}
