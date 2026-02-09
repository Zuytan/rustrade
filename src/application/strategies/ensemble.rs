use super::traits::{AnalysisContext, Signal, TradingStrategy};
use super::{SMCStrategy, StatisticalMomentumStrategy, ZScoreMeanReversionStrategy};
use crate::application::agents::analyst_config::AnalystConfig;
use std::collections::HashMap;
use std::sync::Arc;

/// Ensemble Strategy
///
/// Combines multiple trading strategies and requires consensus for signals.
/// - Analyzes using all child strategies
/// - Only generates signal if voting threshold is met
/// - Confidence is averaged (or weighted by performance when weights provided) from agreeing strategies
#[derive(Clone)]
pub struct EnsembleStrategy {
    strategies: Vec<Arc<dyn TradingStrategy>>,
    voting_threshold: f64, // 0.0 to 1.0 - percentage of strategies that must agree
    /// Optional per-strategy weights (e.g. rolling Sharpe). Key = strategy name. When None, equal vote.
    weights: Option<HashMap<String, f64>>,
}

impl EnsembleStrategy {
    pub fn new(strategies: Vec<Arc<dyn TradingStrategy>>, voting_threshold: f64) -> Self {
        Self {
            strategies,
            voting_threshold: voting_threshold.clamp(0.0, 1.0),
            weights: None,
        }
    }

    /// Build ensemble with performance weights (e.g. rolling Sharpe per strategy name).
    pub fn with_weights(
        strategies: Vec<Arc<dyn TradingStrategy>>,
        voting_threshold: f64,
        weights: HashMap<String, f64>,
    ) -> Self {
        Self {
            strategies,
            voting_threshold: voting_threshold.clamp(0.0, 1.0),
            weights: Some(weights),
        }
    }

    fn weight_for(&self, name: &str) -> f64 {
        self.weights
            .as_ref()
            .and_then(|w| w.get(name).copied())
            .unwrap_or(1.0)
    }

    /// Create an ensemble with majority voting (>50% must agree)
    pub fn majority(strategies: Vec<Arc<dyn TradingStrategy>>) -> Self {
        Self::new(strategies, 0.5)
    }

    /// Create an ensemble requiring unanimous agreement
    pub fn unanimous(strategies: Vec<Arc<dyn TradingStrategy>>) -> Self {
        Self::new(strategies, 1.0)
    }

    /// Create a default ensemble with legacy strategies (deprecated). Prefer `modern_ensemble`.
    pub fn default_ensemble() -> Self {
        Self::modern_ensemble(&AnalystConfig::default())
    }

    /// Modern ensemble: StatisticalMomentum (0.4) + ZScoreMR (0.3) + SMC (0.3), weighted voting >= 0.5.
    pub fn modern_ensemble(config: &AnalystConfig) -> Self {
        let strategies: Vec<Arc<dyn TradingStrategy>> = vec![
            Arc::new(StatisticalMomentumStrategy::new(
                config.stat_momentum_lookback,
                config.stat_momentum_threshold,
                config.stat_momentum_trend_confirmation,
            )),
            Arc::new(ZScoreMeanReversionStrategy::new(
                config.zscore_lookback,
                config.zscore_entry_threshold,
                config.zscore_exit_threshold,
            )),
            Arc::new(SMCStrategy::new(
                config.smc_ob_lookback,
                config.smc_min_fvg_size_pct,
                config.smc_volume_multiplier,
            )),
        ];
        let weights = HashMap::from([
            ("StatMomentum".to_string(), 0.4),
            ("ZScoreMR".to_string(), 0.3),
            ("SMC".to_string(), 0.3),
        ]);
        // Voting threshold 0.50: Requires at least 2 strategies to agree (e.g. StatMomentum + SMC)
        // or strong consensus among all three.
        Self::with_weights(strategies, 0.50, weights)
    }
}

impl TradingStrategy for EnsembleStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        if self.strategies.is_empty() {
            return None;
        }

        let mut buy_votes = 0_usize;
        let mut sell_votes = 0_usize;
        let mut buy_weight = 0.0_f64;
        let mut sell_weight = 0.0_f64;
        let mut buy_confidence_weighted = 0.0_f64;
        let mut sell_confidence_weighted = 0.0_f64;
        let mut buy_reasons = Vec::new();
        let mut sell_reasons = Vec::new();
        let mut total_weight = 0.0_f64;

        for strategy in &self.strategies {
            let w = self.weight_for(strategy.name());
            total_weight += w;
            if let Some(signal) = strategy.analyze(ctx) {
                match signal.side {
                    crate::domain::trading::types::OrderSide::Buy => {
                        buy_votes += 1;
                        buy_weight += w;
                        buy_confidence_weighted += signal.confidence * w;
                        buy_reasons.push(format!("{}: {}", strategy.name(), signal.reason));
                    }
                    crate::domain::trading::types::OrderSide::Sell => {
                        sell_votes += 1;
                        sell_weight += w;
                        sell_confidence_weighted += signal.confidence * w;
                        sell_reasons.push(format!("{}: {}", strategy.name(), signal.reason));
                    }
                }
            }
        }

        let required_weight = total_weight * self.voting_threshold;
        let total_strategies = self.strategies.len();

        // Check for buy consensus (weighted)
        if buy_weight >= required_weight && buy_weight > 0.0 {
            let avg_confidence = buy_confidence_weighted / buy_weight;
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
        if sell_weight >= required_weight && sell_weight > 0.0 {
            let avg_confidence = sell_confidence_weighted / sell_weight;
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
    use crate::application::strategies::legacy::{DualSMAStrategy, MeanReversionStrategy};
    use crate::domain::trading::types::OrderSide;
    use rust_decimal::Decimal;
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
            fast_sma: Decimal::from_f64_retain(fast_sma).unwrap_or(Decimal::ZERO),
            slow_sma: Decimal::from_f64_retain(slow_sma).unwrap_or(Decimal::ZERO),
            trend_sma: dec!(99.0), // Below price to allow buy signals
            rsi: Decimal::from_f64_retain(rsi).unwrap_or(Decimal::ZERO),
            macd_value: dec!(0.5),
            macd_signal: dec!(0.3),
            macd_histogram: dec!(0.2),
            last_macd_histogram: Some(dec!(0.1)),
            atr: dec!(1.0),
            bb_lower: Decimal::from_f64_retain(bb_lower).unwrap_or(Decimal::ZERO),
            bb_middle: dec!(100.0),
            bb_upper: dec!(105.0),
            adx: dec!(30.0),
            has_position,
            position: None,
            timestamp: 0,
            timeframe_features: None,
            candles: VecDeque::new(),
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
    fn test_majority_vote_buy() {
        // Create strategies that will both signal buy
        let strategies: Vec<Arc<dyn TradingStrategy>> = vec![
            Arc::new(DualSMAStrategy::new(20, 60, dec!(0.001))), // Will signal buy if fast > slow
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
            Arc::new(DualSMAStrategy::new(20, 60, dec!(0.001))), // Golden cross buy
            Arc::new(MeanReversionStrategy::new(20, dec!(50.0))), // Needs price < BB lower and RSI < 30
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
            Arc::new(DualSMAStrategy::new(20, 60, dec!(0.001))),
            Arc::new(MeanReversionStrategy::new(20, dec!(50.0))),
        ];

        let ensemble = EnsembleStrategy::unanimous(strategies);

        // Conditions for both: Golden cross AND price < BB lower with RSI < 30
        // DualSMA: fast > slow * (Decimal::ONE + dec!(0.001)) AND price > trend_sma -> buy
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

    #[test]
    fn test_modern_ensemble_creation() {
        let config = AnalystConfig::default();
        let ensemble = EnsembleStrategy::modern_ensemble(&config);
        assert_eq!(ensemble.name(), "Ensemble");
        // modern_ensemble uses with_weights so we cannot easily assert num strategies without exposing;
        // just ensure default_ensemble() returns same type (it delegates to modern_ensemble)
        let default_ens = EnsembleStrategy::default_ensemble();
        assert_eq!(default_ens.name(), "Ensemble");
    }
}
