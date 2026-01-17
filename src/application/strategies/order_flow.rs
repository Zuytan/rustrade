use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::domain::market::order_flow::detect_stacked_imbalances;

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
    pub ofi_threshold: f64,
    /// Number of consecutive OFI values required for stacked imbalance (default: 3)
    pub stacked_count: usize,
    /// Lookback period for volume profile analysis (default: 100)
    pub volume_profile_lookback: usize,
}

impl OrderFlowStrategy {
    pub fn new(ofi_threshold: f64, stacked_count: usize, volume_profile_lookback: usize) -> Self {
        Self {
            ofi_threshold,
            stacked_count,
            volume_profile_lookback,
        }
    }
}

impl Default for OrderFlowStrategy {
    fn default() -> Self {
        Self::new(0.3, 3, 100)
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
            // Additional confirmation: Cumulative Delta rising
            let delta_rising = if ctx.ofi_history.len() >= 5 {
                let recent_avg: f64 = ctx.ofi_history.iter().rev().take(5).sum::<f64>() / 5.0;
                ctx.cumulative_delta > recent_avg
            } else {
                true // Not enough history, allow signal
            };

            // Check if price is near a High Volume Node (support)
            let near_hvn = if let Some(ref profile) = ctx.volume_profile {
                profile
                    .high_volume_nodes
                    .iter()
                    .any(|&hvn| (ctx.price_f64 - hvn).abs() / ctx.price_f64 < 0.02) // Within 2%
            } else {
                true // No volume profile, allow signal
            };

            if delta_rising && ctx.ofi_value > self.ofi_threshold {
                let confidence = if near_hvn { 0.9 } else { 0.7 };
                return Some(
                    Signal::buy(format!(
                        "Stacked Bullish OFI (OFI={:.2}, Delta={:.2}, HVN={})",
                        ctx.ofi_value,
                        ctx.cumulative_delta,
                        if near_hvn { "Yes" } else { "No" }
                    ))
                    .with_confidence(confidence),
                );
            }
        }

        // Bearish stacked imbalances
        if direction == -1 && ctx.has_position {
            // Additional confirmation: Cumulative Delta falling
            let delta_falling = if ctx.ofi_history.len() >= 5 {
                let recent_avg: f64 = ctx.ofi_history.iter().rev().take(5).sum::<f64>() / 5.0;
                ctx.cumulative_delta < recent_avg
            } else {
                true // Not enough history, allow signal
            };

            // Check if price is near a High Volume Node (resistance)
            let near_hvn = if let Some(ref profile) = ctx.volume_profile {
                profile
                    .high_volume_nodes
                    .iter()
                    .any(|&hvn| (ctx.price_f64 - hvn).abs() / ctx.price_f64 < 0.02) // Within 2%
            } else {
                true // No volume profile, allow signal
            };

            if delta_falling && ctx.ofi_value < -self.ofi_threshold {
                let confidence = if near_hvn { 0.9 } else { 0.7 };
                return Some(
                    Signal::sell(format!(
                        "Stacked Bearish OFI (OFI={:.2}, Delta={:.2}, HVN={})",
                        ctx.ofi_value,
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
    use crate::domain::trading::types::OrderSide;
    use crate::domain::market::order_flow::VolumeProfile;
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
                high_volume_nodes: nodes,
                point_of_control: poc,
            }
        });

        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
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
            adx: 0.0,
            has_position,
            timestamp: 0,
            candles: VecDeque::new(),
            rsi_history: VecDeque::new(),
            ofi_value,
            cumulative_delta,
            volume_profile,
            ofi_history: ofi_history.into_iter().collect(),
            timeframe_features: None,
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

        // Price at 100, HVN at 99 (within 2%)
        let ofi_history = vec![0.4, 0.5, 0.6];
        let ctx = create_test_context(0.6, ofi_history, 1.5, false, 100.0, Some(vec![99.0]));

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

        // Price at 100, HVN at 101 (within 2%)
        let ofi_history = vec![-0.4, -0.5, -0.6];
        let ctx = create_test_context(-0.6, ofi_history, -1.5, true, 100.0, Some(vec![101.0]));

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
    fn test_no_sell_without_position() {
        let strategy = OrderFlowStrategy::default();

        // Bearish stack but no position
        let ofi_history = vec![-0.4, -0.5, -0.6];
        let ctx = create_test_context(-0.6, ofi_history, -1.5, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_none(), "Should not sell without position");
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
        let strategy = OrderFlowStrategy::new(0.2, 2, 50); // Lower threshold, 2 stacked

        // 2 consecutive OFI > 0.2
        let ofi_history = vec![0.25, 0.3];
        let ctx = create_test_context(0.3, ofi_history, 0.5, false, 100.0, None);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
    }
}
