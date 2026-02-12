use super::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::market::order_flow::detect_stacked_imbalances;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Order Flow Imbalance (OFI) Strategy
///
/// Trades based on institutional order flow patterns:
/// - Stacked imbalances (consecutive OFI in same direction)
/// - Cumulative Delta trends
/// - High Volume Node support/resistance
///
/// This strategy is designed to capture institutional buying/selling pressure.
#[derive(Debug, Clone)]
pub struct OrderFlowStrategy {
    /// Minimum OFI value to consider significant (default: 0.3)
    pub ofi_threshold: Decimal,
    /// Number of consecutive OFI values required for stacked imbalance (default: 3)
    pub stacked_count: usize,
    /// Lookback period for volume profile analysis (default: 100)
    pub volume_profile_lookback: usize,
}

impl OrderFlowStrategy {
    /// Threshold for "near" a High Volume Node (0.5%)
    const HVN_THRESHOLD: Decimal = dec!(0.005);
    /// Lookback for momentum confirmation
    const MOMENTUM_LOOKBACK: usize = 5;

    pub fn new(
        ofi_threshold: Decimal,
        stacked_count: usize,
        volume_profile_lookback: usize,
    ) -> Self {
        Self {
            ofi_threshold,
            stacked_count,
            volume_profile_lookback,
        }
    }
}

impl Default for OrderFlowStrategy {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self::new(dec!(0.3), 3, 100)
    }
}

impl TradingStrategy for OrderFlowStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // Check for stacked imbalances
        let (is_stacked, direction) =
            detect_stacked_imbalances(&ctx.ofi_history, self.ofi_threshold, self.stacked_count);

        if !is_stacked {
            return None; // No clear institutional pressure
        }

        // Bullish stacked imbalances
        if direction == 1 {
            // Additional confirmation: OFI Momentum rising (acceleration of buying pressure)
            // Skip the most recent value (ofi_value itself) to avoid self-comparison bias
            let ofi_momentum_rising = if ctx.ofi_history.len() > Self::MOMENTUM_LOOKBACK {
                let recent_avg: Decimal = ctx
                    .ofi_history
                    .iter()
                    .rev()
                    .skip(1) // Exclude current ofi_value (last pushed)
                    .take(Self::MOMENTUM_LOOKBACK)
                    .copied()
                    .sum::<Decimal>()
                    / Decimal::from(Self::MOMENTUM_LOOKBACK);
                ctx.ofi_value > recent_avg
            } else {
                ctx.ofi_value > Decimal::ZERO
            };

            // True Cumulative Delta check (Volume Delta accumulation)
            let delta_confirmed = ctx.cumulative_delta > Decimal::ZERO;

            // Check if price is near a High Volume Node (support)
            let near_hvn = if let Some(ref profile) = ctx.volume_profile {
                profile.high_volume_nodes.iter().any(|&hvn| {
                    if ctx.current_price > Decimal::ZERO {
                        (ctx.current_price - hvn).abs() / ctx.current_price < Self::HVN_THRESHOLD
                    } else {
                        false
                    }
                }) // Within 2%
            } else {
                true // No volume profile, allow signal
            };

            if ofi_momentum_rising && delta_confirmed && ctx.ofi_value > self.ofi_threshold {
                let confidence = if near_hvn { 0.9 } else { 0.7 };
                return Some(
                    Signal::buy(format!(
                        "Stacked Bullish OFI (OFI={}, Momentum={}, Delta={}, HVN={})",
                        ctx.ofi_value,
                        if ofi_momentum_rising {
                            "Rising"
                        } else {
                            "Flat"
                        },
                        ctx.cumulative_delta,
                        if near_hvn { "Yes" } else { "No" }
                    ))
                    .with_confidence(confidence),
                );
            }
        }

        // Bearish stacked imbalances
        // Removed has_position check to allow Short Entry
        if direction == -1 {
            // Additional confirmation: OFI Momentum falling
            // Skip the most recent value (ofi_value itself) to avoid self-comparison bias
            let ofi_momentum_falling = if ctx.ofi_history.len() > Self::MOMENTUM_LOOKBACK {
                let recent_avg: Decimal = ctx
                    .ofi_history
                    .iter()
                    .rev()
                    .skip(1) // Exclude current ofi_value (last pushed)
                    .take(Self::MOMENTUM_LOOKBACK)
                    .copied()
                    .sum::<Decimal>()
                    / Decimal::from(Self::MOMENTUM_LOOKBACK);
                ctx.ofi_value < recent_avg
            } else {
                ctx.ofi_value < Decimal::ZERO // Simple negative check
            };

            // True Cumulative Delta check
            let delta_confirmed = ctx.cumulative_delta < Decimal::ZERO;

            // Check if price is near a High Volume Node (resistance)
            let near_hvn = if let Some(ref profile) = ctx.volume_profile {
                profile.high_volume_nodes.iter().any(|&hvn| {
                    if ctx.current_price > Decimal::ZERO {
                        (ctx.current_price - hvn).abs() / ctx.current_price < Self::HVN_THRESHOLD
                    } else {
                        false
                    }
                }) // Within 2%
            } else {
                true // No volume profile, allow signal
            };

            if ofi_momentum_falling && delta_confirmed && ctx.ofi_value < -self.ofi_threshold {
                let confidence = if near_hvn { 0.9 } else { 0.7 };
                return Some(
                    Signal::sell(format!(
                        "Stacked Bearish OFI (OFI={}, Momentum={}, Delta={}, HVN={})",
                        ctx.ofi_value,
                        if ofi_momentum_falling {
                            "Falling"
                        } else {
                            "Flat"
                        },
                        ctx.cumulative_delta,
                        if near_hvn { "Yes" } else { "No" }
                    ))
                    .with_confidence(confidence),
                );
            }
        }

        None
    }

    fn name(&self) -> &str {
        "OrderFlow"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::market::order_flow::VolumeProfile;
    use crate::domain::trading::types::OrderSide;
    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal_macros::dec;
    use std::collections::{HashMap, VecDeque};

    fn create_test_context(
        ofi_value: f64,
        ofi_history: Vec<f64>,
        cumulative_delta: f64,
        has_position: bool,
        price: f64,
        hvns: Option<Vec<f64>>,
    ) -> AnalysisContext {
        let volume_profile = hvns.map(|nodes| {
            let poc = nodes.first().copied().unwrap_or(0.0);
            VolumeProfile {
                levels: HashMap::new(),
                high_volume_nodes: nodes
                    .into_iter()
                    .map(|n| Decimal::from_f64(n).unwrap())
                    .collect(),
                point_of_control: Decimal::from_f64(poc).unwrap(),
            }
        });

        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: Decimal::from_f64(price).unwrap(),
            price_f64: price,
            fast_sma: Some(Decimal::ZERO),
            slow_sma: Some(Decimal::ZERO),
            trend_sma: Some(Decimal::ZERO),
            rsi: Some(dec!(50.0)),
            macd_value: Some(Decimal::ZERO),
            macd_signal: Some(Decimal::ZERO),
            macd_histogram: Some(Decimal::ZERO),
            last_macd_histogram: None,
            atr: Some(Decimal::ONE),
            bb_lower: Some(Decimal::ZERO),
            bb_middle: Some(Decimal::ZERO),
            bb_upper: Some(Decimal::ZERO),
            adx: Some(Decimal::ZERO),
            has_position,
            position: None,
            timestamp: 0,
            candles: VecDeque::new(),
            rsi_history: VecDeque::new(),
            ofi_value: Decimal::from_f64(ofi_value).unwrap(),
            cumulative_delta: Decimal::from_f64(cumulative_delta).unwrap(),
            volume_profile,
            ofi_history: ofi_history
                .into_iter()
                .map(|o| Decimal::from_f64(o).unwrap())
                .collect(),
            hurst_exponent: None,
            skewness: None,
            momentum_normalized: None,
            realized_volatility: None,
            timeframe_features: None,
            feature_set: None,
        }
    }

    #[test]
    fn test_buy_signal_stacked_bullish_imbalances() {
        let strategy = OrderFlowStrategy::default();

        // 3 consecutive bullish OFI values > 0.3
        // Cumulative delta = 1.5, recent avg = (0.4+0.5+0.6+0+0)/5 = 0.3
        // Delta rising: 1.5 > 0.3 ✓
        let ofi_history = vec![0.4, 0.5, 0.6];
        let ctx = create_test_context(0.6, ofi_history, 1.5, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Stacked Bullish OFI"));
        assert_eq!(sig.confidence, 0.9); // Delta rising gives higher confidence
    }

    #[test]
    fn test_buy_signal_with_hvn_support() {
        let strategy = OrderFlowStrategy::default();

        // Price at 100, HVN at 99.6 (within 0.5%)
        let ofi_history = vec![0.4, 0.5, 0.6];
        let ctx = create_test_context(0.6, ofi_history, 1.5, false, 100.0, Some(vec![99.6]));

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert_eq!(sig.confidence, 0.9); // With HVN support
        assert!(sig.reason.contains("HVN=Yes"));
    }

    #[test]
    fn test_sell_signal_stacked_bearish_imbalances() {
        let strategy = OrderFlowStrategy::default();

        // 3 consecutive bearish OFI values < -0.3
        // Cumulative delta = -1.5, recent avg = (-0.4-0.5-0.6+0+0)/5 = -0.3
        // Delta falling: -1.5 < -0.3 ✓
        let ofi_history = vec![-0.4, -0.5, -0.6];
        let ctx = create_test_context(-0.6, ofi_history, -1.5, true, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert!(sig.reason.contains("Stacked Bearish OFI"));
        assert_eq!(sig.confidence, 0.9); // Delta falling gives higher confidence
    }

    #[test]
    fn test_sell_signal_with_hvn_resistance() {
        let strategy = OrderFlowStrategy::default();

        // Price at 100, HVN at 100.4 (within 0.5%)
        let ofi_history = vec![-0.4, -0.5, -0.6];
        let ctx = create_test_context(-0.6, ofi_history, -1.5, true, 100.0, Some(vec![100.4]));

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
        assert_eq!(sig.confidence, 0.9); // With HVN resistance
        assert!(sig.reason.contains("HVN=Yes"));
    }

    #[test]
    fn test_no_signal_without_stacked_imbalances() {
        let strategy = OrderFlowStrategy::default();

        // Mixed OFI values (no stack)
        let ofi_history = vec![0.4, -0.2, 0.5];
        let ctx = create_test_context(0.5, ofi_history, 0.7, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none());
    }

    #[test]
    fn test_no_signal_weak_ofi() {
        let strategy = OrderFlowStrategy::default();

        // Stacked but below threshold (0.2 < 0.3)
        let ofi_history = vec![0.2, 0.2, 0.2];
        let ctx = create_test_context(0.2, ofi_history, 0.5, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none());
    }

    #[test]
    fn test_sell_signal_short_entry() {
        let strategy = OrderFlowStrategy::default();

        // Bearish stack and no position -> Should signal Short Entry
        let ofi_history = vec![-0.4, -0.5, -0.6];
        let ctx = create_test_context(-0.6, ofi_history, -1.5, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some(), "Should signal Sell for Short Entry");
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Sell));
    }

    #[test]
    fn test_insufficient_ofi_history() {
        let strategy = OrderFlowStrategy::default();

        // Only 2 values (need 3 for default stacked_count)
        let ofi_history = vec![0.4, 0.5];
        let ctx = create_test_context(0.5, ofi_history, 1.0, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none());
    }

    #[test]
    fn test_custom_parameters() {
        let strategy = OrderFlowStrategy::new(dec!(0.2), 2, 50); // Lower threshold, 2 stacked

        // 2 consecutive OFI > 0.2
        let ofi_history = vec![0.25, 0.3];
        let ctx = create_test_context(0.3, ofi_history, 0.5, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
    }
}
