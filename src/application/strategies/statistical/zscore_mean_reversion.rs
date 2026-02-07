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
    pub entry_threshold: Decimal, // Typically -2.0 (2 std devs below mean)
    pub exit_threshold: Decimal,  // Typically 0.0 (return to mean)
    pub min_data_points: usize,
}

impl ZScoreMeanReversionStrategy {
    pub fn new(lookback_period: usize, entry_threshold: Decimal, exit_threshold: Decimal) -> Self {
        Self {
            lookback_period,
            entry_threshold,
            exit_threshold,
            min_data_points: lookback_period.max(20),
        }
    }

    /// Calculate Z-Score: (Price - Mean) / StdDev
    fn calculate_zscore(&self, ctx: &AnalysisContext) -> Option<Decimal> {
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

        // Calculate mean and std dev using statrs (f64 boundary for statistical library)
        let data = Data::new(prices);
        let mean = data.mean()?;
        let std_dev = data.std_dev()?;

        if std_dev == 0.0 {
            return None; // Avoid division by zero
        }

        // Z-Score = (Current Price - Mean) / StdDev — core comparison in Decimal for consistency with thresholds
        let mean_d = Decimal::from_f64_retain(mean).unwrap_or(Decimal::ZERO);
        let std_d = Decimal::from_f64_retain(std_dev).unwrap_or(Decimal::ONE);
        Some((ctx.current_price - mean_d) / std_d)
    }
}

impl Default for ZScoreMeanReversionStrategy {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self::new(
            20,         // 20-period lookback
            dec!(-2.0), // Entry at 2 std devs below mean
            dec!(0.0),  // Exit at mean
        )
    }
}

impl TradingStrategy for ZScoreMeanReversionStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let zscore = self.calculate_zscore(ctx)?;

        // BUY: Price significantly below mean (oversold). Confidence scales with Z magnitude.
        if !ctx.has_position && zscore < self.entry_threshold {
            let excess = (zscore.abs() - self.entry_threshold.abs())
                .to_f64()
                .unwrap_or(0.0);
            let confidence = (0.5 + (excess * 0.15)).min(0.95);
            return Some(
                Signal::buy(format!(
                    "Z-Score MR: Price {} is {} std devs below mean (Z={})",
                    ctx.current_price,
                    zscore.abs(),
                    zscore
                ))
                .with_confidence(confidence),
            );
        }

        // SELL: Price returned to mean or above. Confidence scales with distance above mean.
        if ctx.has_position && zscore > self.exit_threshold {
            let distance_above = (zscore - self.exit_threshold).abs().to_f64().unwrap_or(0.0);
            let confidence = (0.5 + (distance_above * 0.10)).min(0.90);
            return Some(
                Signal::sell(format!(
                    "Z-Score MR: Price {} returned to mean (Z={})",
                    ctx.current_price, zscore
                ))
                .with_confidence(confidence),
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
            high: Decimal::from_f64(close).unwrap() * dec!(1.01),
            low: Decimal::from_f64(close).unwrap() * dec!(0.99),
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
        let d_price = Decimal::from_f64(price).unwrap();
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: d_price,
            price_f64: price,
            fast_sma: Decimal::ZERO,
            slow_sma: Decimal::ZERO,
            trend_sma: Decimal::ZERO,
            rsi: dec!(50.0),
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
