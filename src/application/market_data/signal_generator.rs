use crate::application::strategies::{AnalysisContext, TradingStrategy};
use crate::domain::trading::types::{FeatureSet, OrderSide};
use rust_decimal::Decimal;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::info;

pub struct SignalGenerator {
    pub last_was_above: Option<bool>,
}

impl Default for SignalGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator {
    pub fn new() -> Self {
        Self {
            last_was_above: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn generate_signal(
        &self,
        symbol: &str,
        price: Decimal,
        timestamp: i64,
        features: &FeatureSet,
        strategy: &Arc<dyn TradingStrategy>,
        _sma_threshold: f64, // Unused
        has_position: bool,
        previous_macd_histogram: Option<f64>, // Previous MACD histogram for rising/falling detection
        candle_history: &VecDeque<crate::domain::trading::types::Candle>,
        rsi_history: &VecDeque<f64>,
        // OFI parameters
        ofi_value: f64,
        cumulative_delta: f64,
        volume_profile: Option<crate::domain::market::order_flow::VolumeProfile>,
        ofi_history: &VecDeque<f64>,
    ) -> Option<OrderSide> {
        let price_f64 = rust_decimal::prelude::ToPrimitive::to_f64(&price).unwrap_or(0.0);

        // Strategy Logic (Authoritative)
        let analysis_ctx = AnalysisContext {
            symbol: symbol.to_string(),
            current_price: price,
            price_f64,
            fast_sma: features.sma_20.unwrap_or(0.0), // Using SMA 20 as fast
            slow_sma: features.sma_50.unwrap_or(0.0), // Using SMA 50 as slow
            trend_sma: features.sma_200.unwrap_or(0.0),
            rsi: features.rsi.unwrap_or(0.0),
            macd_value: features.macd_line.unwrap_or(0.0),
            macd_signal: features.macd_signal.unwrap_or(0.0),
            macd_histogram: features.macd_hist.unwrap_or(0.0),
            last_macd_histogram: previous_macd_histogram, // Use tracked previous value
            atr: features.atr.unwrap_or(0.0),
            bb_lower: features.bb_lower.unwrap_or(0.0),
            bb_upper: features.bb_upper.unwrap_or(0.0),
            bb_middle: features.bb_middle.unwrap_or(0.0),
            adx: features.adx.unwrap_or(0.0),
            has_position,
            timestamp,
            candles: candle_history.clone(),
            rsi_history: rsi_history.clone(),
            // OFI fields from parameters
            ofi_value,
            cumulative_delta,
            volume_profile,
            ofi_history: ofi_history.clone(),

            // Advanced Statistical Features (Phase 2)
            // Advanced Statistical Features (Phase 2)
            hurst_exponent: features.hurst_exponent,
            skewness: features.skewness,
            momentum_normalized: features.momentum_normalized,
            realized_volatility: features.realized_volatility,
            timeframe_features: None, // Will be populated by Analyst when multi-timeframe is enabled
        };

        if let Some(strategy_signal) = strategy.analyze(&analysis_ctx) {
            info!(
                "SignalGenerator [{}]: {} - {}",
                strategy.name(),
                symbol,
                strategy_signal.reason
            );
            return Some(strategy_signal.side);
        }

        None
    }

    // Legacy method removed/unused
    #[allow(dead_code)]
    fn check_sma_crossover(&mut self, features: &FeatureSet, threshold: f64) -> Option<OrderSide> {
        // Keep code for now to avoid breaking other legacy refs if any, but unused here
        let fast = features.ema_fast.or(features.sma_20)?;
        let slow = features.ema_slow.or(features.sma_50)?;

        let is_definitively_above = fast > slow * (1.0 + threshold);
        let is_definitively_below = fast < slow * (1.0 - threshold);

        match self.last_was_above {
            None => {
                if is_definitively_above {
                    self.last_was_above = Some(true);
                } else if is_definitively_below {
                    self.last_was_above = Some(false);
                }
                None
            }
            Some(true) => {
                if is_definitively_below {
                    self.last_was_above = Some(false);
                    Some(OrderSide::Sell)
                } else {
                    None
                }
            }
            Some(false) => {
                if is_definitively_above {
                    self.last_was_above = Some(true);
                    Some(OrderSide::Buy)
                } else {
                    None
                }
            }
        }
    }
}
