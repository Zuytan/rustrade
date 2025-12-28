use crate::application::strategies::{AnalysisContext, TradingStrategy};
use crate::domain::trading::types::{FeatureSet, OrderSide};
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::info;

pub struct SignalGenerator {
    pub last_was_above: Option<bool>,
}

impl SignalGenerator {
    pub fn new() -> Self {
        Self { last_was_above: None }
    }

    pub fn generate_signal(
        &mut self,
        symbol: &str,
        price: Decimal,
        timestamp: i64,
        features: &FeatureSet,
        strategy: &Arc<dyn TradingStrategy>,
        sma_threshold: f64,
        has_position: bool,
    ) -> Option<OrderSide> {
        let price_f64 = rust_decimal::prelude::ToPrimitive::to_f64(&price).unwrap_or(0.0);

        // 1. SMA Crossover Logic (Legacy/Standard)
        let mut signal = self.check_sma_crossover(features, sma_threshold);

        // 2. Strategy Logic
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
            last_macd_histogram: Some(features.macd_hist.unwrap_or(0.0)),
            atr: features.atr.unwrap_or(0.0),
            bb_lower: features.bb_lower.unwrap_or(0.0),
            bb_upper: features.bb_upper.unwrap_or(0.0),
            bb_middle: features.bb_middle.unwrap_or(0.0),
            has_position,
            timestamp,
        };

        if let Some(strategy_signal) = strategy.analyze(&analysis_ctx) {
            info!(
                "SignalGenerator [{}]: {} - {}",
                strategy.name(),
                symbol,
                strategy_signal.reason
            );
            signal = Some(strategy_signal.side);
        }

        signal
    }

    fn check_sma_crossover(&mut self, features: &FeatureSet, threshold: f64) -> Option<OrderSide> {
        let fast = features.sma_20?;
        let slow = features.sma_50?;

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
