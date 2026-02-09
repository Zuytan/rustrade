use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::application::strategies::legacy::advanced::{
    AdvancedTripleFilterConfig, AdvancedTripleFilterStrategy,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Configuration for Dynamic Regime Strategy
///
/// These parameters are derived from RiskAppetite when available
#[derive(Debug, Clone)]
pub struct DynamicRegimeConfig {
    pub fast_period: usize,
    pub slow_period: usize,
    pub sma_threshold: Decimal,
    pub trend_sma_period: usize,
    pub rsi_threshold: Decimal,
    pub trend_divergence_threshold: Decimal,
    // Risk-appetite adaptive parameters
    pub signal_confirmation_bars: usize,
    pub macd_requires_rising: bool,
    pub trend_tolerance_pct: Decimal,
    pub macd_min_threshold: Decimal,
    pub adx_threshold: Decimal,
}

impl Default for DynamicRegimeConfig {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            trend_divergence_threshold: dec!(0.005),
            signal_confirmation_bars: 1,
            macd_requires_rising: true,
            trend_tolerance_pct: Decimal::ZERO,
            macd_min_threshold: Decimal::ZERO,
            adx_threshold: dec!(25.0),
        }
    }
}

/// Dynamic Regime Detection Strategy
///
/// Adapts behavior based on market regime:
/// - Strong Trend: Looser filters, hold through pullbacks
/// - Choppy/Range-bound: Strict filters (uses Advanced strategy)
#[derive(Debug, Clone)]
pub struct DynamicRegimeStrategy {
    advanced_strategy: AdvancedTripleFilterStrategy,
    #[allow(dead_code)]
    trend_divergence_threshold: Decimal, // Legacy field, now using ADX-based regime detection
}

impl DynamicRegimeStrategy {
    /// Creates a new DynamicRegimeStrategy with full configuration
    ///
    /// Use this constructor when you have risk_appetite parameters available
    pub fn with_config(config: DynamicRegimeConfig) -> Self {
        Self {
            advanced_strategy: AdvancedTripleFilterStrategy::new(AdvancedTripleFilterConfig {
                fast_period: config.fast_period,
                slow_period: config.slow_period,
                sma_threshold: config.sma_threshold,
                trend_sma_period: config.trend_sma_period,
                rsi_threshold: config.rsi_threshold,
                signal_confirmation_bars: config.signal_confirmation_bars,
                macd_requires_rising: config.macd_requires_rising,
                trend_tolerance_pct: config.trend_tolerance_pct,
                macd_min_threshold: config.macd_min_threshold,
                adx_threshold: config.adx_threshold,
            }),
            trend_divergence_threshold: config.trend_divergence_threshold,
        }
    }

    /// Creates a new DynamicRegimeStrategy with basic parameters (legacy compatibility)
    ///
    /// Deprecated: Use with_config() for proper risk_appetite support
    #[deprecated(note = "Use with_config() for proper risk_appetite support")]
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        sma_threshold: Decimal,
        trend_sma_period: usize,
        rsi_threshold: Decimal,
        trend_divergence_threshold: Decimal,
    ) -> Self {
        Self::with_config(DynamicRegimeConfig {
            fast_period,
            slow_period,
            sma_threshold,
            trend_sma_period,
            rsi_threshold,
            trend_divergence_threshold,
            ..Default::default()
        })
    }

    fn detect_regime(&self, ctx: &AnalysisContext) -> MarketRegime {
        // Use highest available timeframe ADX for more reliable regime detection
        // Higher timeframes give better signal for overall market regime
        let adx = ctx.get_highest_timeframe_adx();

        // ADX-based regime detection (more reliable than SMA divergence)
        // ADX > threshold = Strong Trend, otherwise Choppy
        if adx > self.advanced_strategy.adx_threshold {
            // Check trend direction using price vs trend_sma
            if ctx.current_price > ctx.trend_sma {
                MarketRegime::StrongTrendUp
            } else {
                MarketRegime::StrongTrendDown
            }
        } else {
            MarketRegime::Choppy
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MarketRegime {
    StrongTrendUp,
    StrongTrendDown,
    Choppy,
}

impl TradingStrategy for DynamicRegimeStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let regime = self.detect_regime(ctx);

        match regime {
            MarketRegime::StrongTrendUp => {
                // In strong UPTREND, be more permissive for BUYS
                if ctx.fast_sma > ctx.slow_sma * (Decimal::ONE + dec!(0.001)) {
                    // Golden cross
                    if ctx.current_price > ctx.trend_sma {
                        return Some(Signal::buy(
                            "Dynamic (Trend Up): Strong uptrend detected, buying above Trend SMA"
                                .to_string(),
                        ));
                    }
                }
                // Suppress sells unless trend breaks significantly or death cross
                else if ctx.fast_sma < ctx.slow_sma * (Decimal::ONE - dec!(0.001))
                    && ctx.has_position
                {
                    if ctx.current_price < ctx.trend_sma {
                        return Some(Signal::sell(
                            "Dynamic (Trend Up): Trend broken, exiting".to_string(),
                        ));
                    }
                }
                None
            }
            MarketRegime::StrongTrendDown => {
                // In strong DOWNTREND, avoid buying even if Golden Cross occurs
                // (unless we add Shorting logic later)
                if ctx.has_position && ctx.current_price < ctx.trend_sma {
                    // Aggressive exit if we somehow have a position
                    return Some(Signal::sell(
                        "Dynamic (Trend Down): Strong downtrend, exiting".to_string(),
                    ));
                }
                None
            }
            MarketRegime::Choppy => {
                // In choppy markets, use strict Advanced filters
                self.advanced_strategy.analyze(ctx).map(|mut sig| {
                    sig.reason = format!("Dynamic (Choppy): {}", sig.reason);
                    sig
                })
            }
        }
    }

    fn name(&self) -> &str {
        "DynamicRegime"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::trading::types::OrderSide;
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;

    fn create_test_context(
        fast_sma: Decimal,
        slow_sma: Decimal,
        price: Decimal,
        trend_sma: Decimal,
        has_position: bool,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: price,
            price_f64: 0.0,
            fast_sma,
            slow_sma,
            trend_sma,
            rsi: dec!(50.0),
            macd_value: dec!(0.5),
            macd_signal: dec!(0.3),
            macd_histogram: dec!(0.2),
            last_macd_histogram: Some(dec!(0.1)),
            atr: Decimal::ONE,
            bb_lower: Decimal::ZERO,
            bb_middle: Decimal::ZERO,
            bb_upper: Decimal::ZERO,
            adx: dec!(30.0), // Strong trend for dynamic strategy tests
            has_position,
            position: None,
            timestamp: 0,
            timeframe_features: None,
            candles: std::collections::VecDeque::new(),
            rsi_history: std::collections::VecDeque::new(),
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
    fn test_strong_trend_buy_signal() {
        let strategy = DynamicRegimeStrategy::with_config(DynamicRegimeConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            trend_divergence_threshold: dec!(0.005),
            ..Default::default()
        });
        // Large divergence = strong trend
        let ctx = create_test_context(dec!(105.0), dec!(100.0), dec!(110.0), dec!(95.0), false);

        let signal = strategy.analyze(&ctx);

        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Dynamic (Trend Up)"));
    }

    #[test]
    fn test_strong_trend_hold_through_pullback() {
        let strategy = DynamicRegimeStrategy::with_config(DynamicRegimeConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            trend_divergence_threshold: dec!(0.005),
            ..Default::default()
        });
        // Large divergence but death cross with price still above trend
        let mut ctx = create_test_context(dec!(98.0), dec!(100.0), dec!(102.0), dec!(95.0), true);
        ctx.has_position = true;

        let signal = strategy.analyze(&ctx);

        // Should NOT sell - holding through pullback
        assert!(
            signal.is_none(),
            "Should hold through pullback in strong trend"
        );
    }

    #[test]
    fn test_choppy_uses_advanced_filters() {
        let strategy = DynamicRegimeStrategy::with_config(DynamicRegimeConfig {
            fast_period: 20,
            slow_period: 60,
            sma_threshold: dec!(0.001),
            trend_sma_period: 200,
            rsi_threshold: dec!(75.0),
            trend_divergence_threshold: dec!(0.005),
            ..Default::default()
        });
        // Small divergence = choppy market, and override ADX to be low
        let mut ctx = create_test_context(dec!(100.2), dec!(100.0), dec!(105.0), dec!(95.0), false);
        ctx.adx = dec!(20.0); // Low ADX = choppy market

        let signal = strategy.analyze(&ctx);

        // In choppy, uses Advanced filters which would reject this
        // (MACD too weak, etc.)
        if let Some(sig) = signal {
            assert!(sig.reason.contains("Dynamic (Choppy)"));
        }
    }
}
