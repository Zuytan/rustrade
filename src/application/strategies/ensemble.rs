use super::traits::{AnalysisContext, Signal, TradingStrategy};
use std::sync::Arc;

/// Ensemble Strategy
///
/// Combines multiple trading strategies and requires consensus for signals.
/// - Analyzes using all child strategies
/// - Only generates signal if voting threshold is met
/// - Confidence is averaged from agreeing strategies
#[derive(Clone)]
pub struct EnsembleStrategy {
    strategies: Vec<Arc<dyn TradingStrategy>>,
    voting_threshold: f64, // 0.0 to 1.0 - percentage of strategies that must agree
}

impl EnsembleStrategy {
    pub fn new(strategies: Vec<Arc<dyn TradingStrategy>>, voting_threshold: f64) -> Self {
        Self {
            strategies,
            voting_threshold: voting_threshold.clamp(0.0, 1.0),
        }
    }

    /// Create an ensemble with majority voting (>50% must agree)
    pub fn majority(strategies: Vec<Arc<dyn TradingStrategy>>) -> Self {
        Self::new(strategies, 0.5)
    }

    /// Create an ensemble requiring unanimous agreement
    pub fn unanimous(strategies: Vec<Arc<dyn TradingStrategy>>) -> Self {
        Self::new(strategies, 1.0)
    }

    /// Create a default ensemble with common strategies
    pub fn default_ensemble() -> Self {
        use super::{
            AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy, DualSMAStrategy,
            MeanReversionStrategy,
        };

        let strategies: Vec<Arc<dyn TradingStrategy>> = vec![
            Arc::new(DualSMAStrategy::new(20, 60, 0.001)),
            Arc::new(AdvancedTripleFilterStrategy::new(
                AdvancedTripleFilterConfig {
                    fast_period: 20,
                    slow_period: 60,
                    sma_threshold: 0.001,
                    trend_sma_period: 50,
                    rsi_threshold: 75.0,
                    signal_confirmation_bars: 1,
                    macd_requires_rising: true,
                    trend_tolerance_pct: 0.0,
                    macd_min_threshold: 0.0,
                    adx_threshold: 25.0,
                },
            )),
            Arc::new(MeanReversionStrategy::new(20, 50.0)),
        ];

        Self::majority(strategies)
    }
}

impl TradingStrategy for EnsembleStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if self.strategies.is_empty() {
            return None;
        }

        let mut buy_votes = 0;
        let mut sell_votes = 0;
        let mut buy_confidence_sum = 0.0;
        let mut sell_confidence_sum = 0.0;
        let mut buy_reasons = Vec::new();
        let mut sell_reasons = Vec::new();

        for strategy in &self.strategies {
            if let Some(signal) = strategy.analyze(ctx) {
                match signal.side {
                    crate::domain::trading::types::OrderSide::Buy => {
                        buy_votes += 1;
                        buy_confidence_sum += signal.confidence;
                        buy_reasons.push(format!("{}: {}", strategy.name(), signal.reason));
                    }
                    crate::domain::trading::types::OrderSide::Sell => {
                        sell_votes += 1;
                        sell_confidence_sum += signal.confidence;
                        sell_reasons.push(format!("{}: {}", strategy.name(), signal.reason));
                    }
                }
            }
        }

        let total_strategies = self.strategies.len();
        let required_votes = (total_strategies as f64 * self.voting_threshold).ceil() as usize;

        // Check for buy consensus
        if buy_votes >= required_votes && buy_votes > 0 {
            let avg_confidence = buy_confidence_sum / buy_votes as f64;
            return Some(
                Signal::buy(format!(
                    "Ensemble ({}/{} agree): {}",
                    buy_votes,
                    total_strategies,
                    buy_reasons.join("; ")
                ))
                .with_confidence(avg_confidence),
            );
        }

        // Check for sell consensus
        if sell_votes >= required_votes && sell_votes > 0 {
            let avg_confidence = sell_confidence_sum / sell_votes as f64;
            return Some(
                Signal::sell(format!(
                    "Ensemble ({}/{} agree): {}",
                    sell_votes,
                    total_strategies,
                    sell_reasons.join("; ")
                ))
                .with_confidence(avg_confidence),
            );
        }

        None
    }

    fn name(&self) -> &str {
        "Ensemble"
    }
}

// Implement Debug manually since Arc<dyn TradingStrategy> doesn't impl Debug
impl std::fmt::Debug for EnsembleStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnsembleStrategy")
            .field("num_strategies", &self.strategies.len())
            .field("voting_threshold", &self.voting_threshold)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::strategies::{DualSMAStrategy, MeanReversionStrategy};
    use crate::domain::trading::types::OrderSide;
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;

    fn create_context(
        fast_sma: f64,
        slow_sma: f64,
        rsi: f64,
        bb_lower: f64,
        price: f64,
        has_position: bool,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
            price_f64: price,
            fast_sma,
            slow_sma,
            trend_sma: 99.0, // Below price to allow buy signals
            rsi,
            macd_value: 0.5,
            macd_signal: 0.3,
            macd_histogram: 0.2,
            last_macd_histogram: Some(0.1),
            atr: 1.0,
            bb_lower,
            bb_middle: 100.0,
            bb_upper: 105.0,
            adx: 30.0,
            has_position,
            timestamp: 0,
            timeframe_features: None,
            candles: VecDeque::new(),
        }
    }

    #[test]
    fn test_majority_vote_buy() {
        // Create strategies that will both signal buy
        let strategies: Vec<Arc<dyn TradingStrategy>> = vec![
            Arc::new(DualSMAStrategy::new(20, 60, 0.001)), // Will signal buy if fast > slow
        ];

        let ensemble = EnsembleStrategy::majority(strategies);

        // Golden cross: fast > slow
        let ctx = create_context(105.0, 100.0, 50.0, 95.0, 102.0, false);

        let signal = ensemble.analyze(&ctx);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Ensemble"));
    }

    #[test]
    fn test_no_signal_when_threshold_not_met() {
        // Create two strategies with different triggers
        let strategies: Vec<Arc<dyn TradingStrategy>> = vec![
            Arc::new(DualSMAStrategy::new(20, 60, 0.001)), // Golden cross buy
            Arc::new(MeanReversionStrategy::new(20, 50.0)), // Needs price < BB lower and RSI < 30
        ];

        let ensemble = EnsembleStrategy::unanimous(strategies); // Requires both

        // Only DualSMA will trigger (golden cross), MeanReversion won't (RSI not oversold)
        let ctx = create_context(105.0, 100.0, 50.0, 95.0, 102.0, false);

        let signal = ensemble.analyze(&ctx);
        assert!(
            signal.is_none(),
            "Should not signal without unanimous agreement"
        );
    }

    #[test]
    fn test_unanimous_vote() {
        // Create strategies that will all signal buy under certain conditions
        let strategies: Vec<Arc<dyn TradingStrategy>> = vec![
            Arc::new(DualSMAStrategy::new(20, 60, 0.001)),
            Arc::new(MeanReversionStrategy::new(20, 50.0)),
        ];

        let ensemble = EnsembleStrategy::unanimous(strategies);

        // Conditions for both: Golden cross AND price < BB lower with RSI < 30
        // DualSMA: fast > slow * 1.001 AND price > trend_sma -> buy
        // MeanReversion: price < bb_lower AND rsi < 30 -> buy
        let ctx = create_context(
            105.0, // fast_sma > slow_sma
            100.0, // slow_sma
            25.0,  // RSI < 30 (oversold)
            101.0, // bb_lower (set above price to trigger mean reversion)
            100.0, // price > trend_sma (99) for DualSMA, < bb_lower for MeanReversion
            false,
        );

        let signal = ensemble.analyze(&ctx);
        // Both should agree on buy
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("2/2 agree"));
    }

    #[test]
    fn test_empty_ensemble() {
        let ensemble = EnsembleStrategy::new(vec![], 0.5);
        let ctx = create_context(105.0, 100.0, 50.0, 95.0, 102.0, false);

        let signal = ensemble.analyze(&ctx);
        assert!(signal.is_none());
    }
}
