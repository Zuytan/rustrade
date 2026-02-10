use crate::domain::trading::types::Candle;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::{HashMap, VecDeque};

/// Order Flow Imbalance - measures net buy/sell pressure
///
/// OFI is calculated from volume analysis to detect institutional order flow.
/// Positive values indicate buying pressure, negative values indicate selling pressure.
#[derive(Debug, Clone)]
pub struct OrderFlowImbalance {
    /// Net imbalance value (-1.0 to +1.0)
    pub value: Decimal,
    /// Aggressive buy volume
    pub buy_volume: Decimal,
    /// Aggressive sell volume
    pub sell_volume: Decimal,
    /// Timestamp of the measurement
    pub timestamp: i64,
}

/// Cumulative Delta - running sum of aggressive buy/sell volume
///
/// Tracks the cumulative difference between buying and selling pressure over time.
/// Divergences between price and cumulative delta can signal reversals.
#[derive(Debug, Clone)]
pub struct CumulativeDelta {
    /// Current cumulative delta value
    pub value: Decimal,
    /// Historical delta values for divergence detection
    pub history: VecDeque<Decimal>,
}

impl CumulativeDelta {
    pub fn new() -> Self {
        Self {
            value: Decimal::ZERO,
            history: VecDeque::with_capacity(20),
        }
    }
}

impl Default for CumulativeDelta {
    fn default() -> Self {
        Self::new()
    }
}

/// Volume Profile - distribution of volume by price level
///
/// Shows where the most trading activity occurred, identifying support/resistance zones.
#[derive(Debug, Clone)]
pub struct VolumeProfile {
    /// Price level (rounded to nearest integer) -> Total volume at that level
    pub levels: HashMap<i64, Decimal>,
    /// High Volume Nodes - prices with significant volume (support/resistance)
    pub high_volume_nodes: Vec<Decimal>,
    /// Point of Control - price level with the highest volume
    pub point_of_control: Decimal,
}

/// Calculate Order Flow Imbalance from recent candles
///
/// Uses a simplified heuristic based on candle body and volume:
/// - Green candles (close > open) contribute to buy volume
/// - Red candles (close < open) contribute to sell volume
/// - OFI = (buy_volume - sell_volume) / total_volume
///
/// # Arguments
/// * `candles` - Recent candle history (typically last 5-10 candles)
///
/// # Returns
/// OrderFlowImbalance with value between -1.0 and +1.0
pub fn calculate_ofi(candles: &VecDeque<Candle>) -> OrderFlowImbalance {
    if candles.is_empty() {
        return OrderFlowImbalance {
            value: Decimal::ZERO,
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            timestamp: 0,
        };
    }

    let mut buy_volume = Decimal::ZERO;
    let mut sell_volume = Decimal::ZERO;

    // Analyze recent candles (last 5 for short-term OFI)
    let lookback = candles.len().min(5);
    let start_idx = candles.len().saturating_sub(lookback);

    for candle in candles.iter().skip(start_idx) {
        let close = candle.close;
        let open = candle.open;
        let volume = candle.volume;
        let range = candle.high - candle.low;
        let body = (close - open).abs();

        // Body ratio: proportion of the candle that is body vs wick
        // A full-body candle (no wicks) = 1.0, a doji = ~0.0
        use rust_decimal_macros::dec;
        let body_ratio = if range > Decimal::ZERO {
            body / range
        } else {
            dec!(0.5) // No range = neutral
        };

        if close > open {
            // Bullish candle: body portion is buying, wick portion is selling pressure
            buy_volume += volume * body_ratio;
            sell_volume += volume * (Decimal::ONE - body_ratio);
        } else if close < open {
            // Bearish candle: body portion is selling, wick portion is buying pressure
            sell_volume += volume * body_ratio;
            buy_volume += volume * (Decimal::ONE - body_ratio);
        } else {
            // Doji - split volume
            buy_volume += volume / dec!(2.0);
            sell_volume += volume / dec!(2.0);
        }
    }

    let total_volume = buy_volume + sell_volume;
    let ofi_value = if total_volume > Decimal::ZERO {
        (buy_volume - sell_volume) / total_volume
    } else {
        Decimal::ZERO
    };

    let last_candle = candles
        .back()
        .expect("candles verified non-empty at function start");

    use rust_decimal_macros::dec;
    OrderFlowImbalance {
        value: ofi_value.clamp(dec!(-1.0), Decimal::ONE),
        buy_volume,
        sell_volume,
        timestamp: last_candle.timestamp,
    }
}

/// Update cumulative delta with new OFI value
///
/// # Arguments
/// * `delta` - Mutable reference to CumulativeDelta state
/// * `ofi_value` - New OFI value to add to cumulative sum
pub fn update_cumulative_delta(delta: &mut CumulativeDelta, ofi_value: Decimal) {
    delta.value += ofi_value;
    delta.history.push_back(delta.value);

    // Keep only last 20 values
    if delta.history.len() > 20 {
        delta.history.pop_front();
    }
}

/// Build volume profile from candle history
///
/// Groups volume by price level to identify high-volume nodes (HVNs)
/// which act as support/resistance zones.
///
/// # Arguments
/// * `candles` - Candle history
/// * `lookback` - Number of recent candles to analyze
///
/// # Returns
/// VolumeProfile with levels, HVNs, and point of control
pub fn build_volume_profile(candles: &VecDeque<Candle>, lookback: usize) -> VolumeProfile {
    let mut levels: HashMap<i64, Decimal> = HashMap::new();

    let start_idx = candles.len().saturating_sub(lookback);

    for candle in candles.iter().skip(start_idx) {
        // Use close price as representative price level
        let price = candle.close;
        let price_level = price.round_dp(0).to_i64().unwrap_or(0); // Round to nearest integer
        let volume = candle.volume;

        *levels.entry(price_level).or_insert(Decimal::ZERO) += volume;
    }

    // Find point of control (highest volume level)
    let poc = levels
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(price, _)| Decimal::from(*price))
        .unwrap_or(Decimal::ZERO);

    // Identify high volume nodes (top 20% of volume levels)
    let mut volume_vec: Vec<(i64, Decimal)> = levels.iter().map(|(k, v)| (*k, *v)).collect();
    volume_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let hvn_count = (volume_vec.len() as f64 * 0.2).ceil() as usize;
    let high_volume_nodes: Vec<Decimal> = volume_vec
        .iter()
        .take(hvn_count)
        .map(|(price, _)| Decimal::from(*price))
        .collect();

    VolumeProfile {
        levels,
        high_volume_nodes,
        point_of_control: poc,
    }
}

/// Detect stacked imbalances (consecutive OFI values in same direction)
///
/// Stacked imbalances indicate sustained institutional pressure.
///
/// # Arguments
/// * `ofi_history` - Recent OFI values
/// * `threshold` - Minimum OFI value to consider significant (default: 0.2)
/// * `min_count` - Minimum consecutive count (default: 3)
///
/// # Returns
/// (is_stacked, direction) where direction is 1 for bullish, -1 for bearish
pub fn detect_stacked_imbalances(
    ofi_history: &VecDeque<Decimal>,
    threshold: Decimal,
    min_count: usize,
) -> (bool, i8) {
    if ofi_history.len() < min_count {
        return (false, 0);
    }

    // Check last N values
    let recent: Vec<Decimal> = ofi_history.iter().rev().take(min_count).copied().collect();

    // Check for bullish stack
    let bullish_stack = recent.iter().all(|&ofi| ofi > threshold);
    if bullish_stack {
        return (true, 1);
    }

    // Check for bearish stack
    let bearish_stack = recent.iter().all(|&ofi| ofi < -threshold);
    if bearish_stack {
        return (true, -1);
    }

    (false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    use rust_decimal_macros::dec;

    fn create_candle(open: f64, close: f64, volume: f64, timestamp: i64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(open).unwrap(),
            high: Decimal::from_f64_retain(close.max(open)).unwrap(),
            low: Decimal::from_f64_retain(close.min(open)).unwrap(),
            close: Decimal::from_f64_retain(close).unwrap(),
            volume: Decimal::from_f64_retain(volume).unwrap(),
            timestamp,
        }
    }

    #[test]
    fn test_calculate_ofi_bullish() {
        let mut candles = VecDeque::new();
        // 5 bullish candles (close > open)
        for i in 0..5 {
            candles.push_back(create_candle(100.0, 105.0, 1000.0, i));
        }

        let ofi = calculate_ofi(&candles);

        assert!(
            ofi.value > Decimal::ZERO,
            "OFI should be positive for bullish candles"
        );
        use rust_decimal_macros::dec;
        assert_eq!(
            ofi.value,
            Decimal::ONE,
            "OFI should be 1.0 for all bullish candles"
        );
        assert_eq!(ofi.buy_volume, dec!(5000.0));
        assert_eq!(ofi.sell_volume, Decimal::ZERO);
    }

    #[test]
    fn test_calculate_ofi_bearish() {
        let mut candles = VecDeque::new();
        // 5 bearish candles (close < open)
        for i in 0..5 {
            candles.push_back(create_candle(105.0, 100.0, 1000.0, i));
        }

        let ofi = calculate_ofi(&candles);

        assert!(
            ofi.value < Decimal::ZERO,
            "OFI should be negative for bearish candles"
        );
        use rust_decimal_macros::dec;
        assert_eq!(
            ofi.value,
            dec!(-1.0),
            "OFI should be -1.0 for all bearish candles"
        );
        assert_eq!(ofi.buy_volume, Decimal::ZERO);
        assert_eq!(ofi.sell_volume, dec!(5000.0));
    }

    #[test]
    fn test_calculate_ofi_mixed() {
        let mut candles = VecDeque::new();
        // 3 bullish, 2 bearish
        candles.push_back(create_candle(100.0, 105.0, 1000.0, 0));
        candles.push_back(create_candle(100.0, 105.0, 1000.0, 1));
        candles.push_back(create_candle(100.0, 105.0, 1000.0, 2));
        candles.push_back(create_candle(105.0, 100.0, 1000.0, 3));
        candles.push_back(create_candle(105.0, 100.0, 1000.0, 4));

        let ofi = calculate_ofi(&candles);

        // Net: 3000 buy - 2000 sell = 1000 / 5000 = 0.2
        use rust_decimal_macros::dec;
        assert_eq!(ofi.value, dec!(0.2));
        assert_eq!(ofi.buy_volume, dec!(3000.0));
        assert_eq!(ofi.sell_volume, dec!(2000.0));
    }

    #[test]
    fn test_calculate_ofi_empty() {
        let candles = VecDeque::new();
        let ofi = calculate_ofi(&candles);

        assert_eq!(ofi.value, Decimal::ZERO);
        assert_eq!(ofi.buy_volume, Decimal::ZERO);
        assert_eq!(ofi.sell_volume, Decimal::ZERO);
    }

    #[test]
    fn test_cumulative_delta_accumulation() {
        let mut delta = CumulativeDelta::new();

        use rust_decimal_macros::dec;
        update_cumulative_delta(&mut delta, dec!(0.5));
        assert!((delta.value - dec!(0.5)).abs() < dec!(1e-10));
        assert_eq!(delta.history.len(), 1);

        update_cumulative_delta(&mut delta, dec!(0.3));
        assert!((delta.value - dec!(0.8)).abs() < dec!(1e-10));
        assert_eq!(delta.history.len(), 2);

        update_cumulative_delta(&mut delta, dec!(-0.2));
        assert!((delta.value - dec!(0.6)).abs() < dec!(1e-10));
        assert_eq!(delta.history.len(), 3);
    }

    #[test]
    fn test_cumulative_delta_history_limit() {
        let mut delta = CumulativeDelta::new();

        use rust_decimal_macros::dec;
        // Add 25 values (should keep only last 20)
        for i in 0..25 {
            update_cumulative_delta(&mut delta, dec!(0.1) * Decimal::from(i));
        }

        assert_eq!(delta.history.len(), 20);
        // First value should be cumulative sum from 0 to 5: 0+0.1+0.2+0.3+0.4+0.5 = 1.5
        assert!((delta.history[0] - dec!(1.5)).abs() < dec!(1e-10));
    }

    #[test]
    fn test_volume_profile_hvn_detection() {
        let mut candles = VecDeque::new();

        // Create candles with clustering around 100 and 110
        for i in 0..10 {
            candles.push_back(create_candle(100.0, 100.0, 1000.0, i));
        }
        for i in 10..15 {
            candles.push_back(create_candle(110.0, 110.0, 500.0, i));
        }
        for i in 15..17 {
            candles.push_back(create_candle(105.0, 105.0, 200.0, i));
        }

        let profile = build_volume_profile(&candles, 20);

        // Point of control should be at 100 (highest volume)
        use rust_decimal_macros::dec;
        assert_eq!(profile.point_of_control, dec!(100.0));

        // Should have HVNs
        assert!(!profile.high_volume_nodes.is_empty());
        assert!(profile.high_volume_nodes.contains(&dec!(100.0)));
    }

    #[test]
    fn test_detect_stacked_imbalances_bullish() {
        let mut ofi_history = VecDeque::new();
        use rust_decimal_macros::dec;
        ofi_history.push_back(dec!(0.3));
        ofi_history.push_back(dec!(0.4));
        ofi_history.push_back(dec!(0.5));

        let (is_stacked, direction) = detect_stacked_imbalances(&ofi_history, dec!(0.2), 3);

        assert!(is_stacked);
        assert_eq!(direction, 1);
    }

    #[test]
    fn test_detect_stacked_imbalances_bearish() {
        let mut ofi_history = VecDeque::new();
        use rust_decimal_macros::dec;
        ofi_history.push_back(dec!(-0.3));
        ofi_history.push_back(dec!(-0.4));
        ofi_history.push_back(dec!(-0.5));

        let (is_stacked, direction) = detect_stacked_imbalances(&ofi_history, dec!(0.2), 3);

        assert!(is_stacked);
        assert_eq!(direction, -1);
    }

    #[test]
    fn test_detect_stacked_imbalances_no_stack() {
        let mut ofi_history = VecDeque::new();
        use rust_decimal_macros::dec;
        ofi_history.push_back(dec!(0.3));
        ofi_history.push_back(dec!(-0.2));
        ofi_history.push_back(dec!(0.4));

        let (is_stacked, _direction) = detect_stacked_imbalances(&ofi_history, dec!(0.2), 3);

        assert!(!is_stacked);
    }

    #[test]
    fn test_detect_stacked_imbalances_insufficient_data() {
        let mut ofi_history = VecDeque::new();
        ofi_history.push_back(dec!(0.3));
        ofi_history.push_back(dec!(0.4));

        let (is_stacked, _direction) = detect_stacked_imbalances(&ofi_history, dec!(0.2), 3);

        assert!(!is_stacked);
    }
}
