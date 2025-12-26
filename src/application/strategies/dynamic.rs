use super::traits::{AnalysisContext, Signal, TradingStrategy};
use super::advanced::AdvancedTripleFilterStrategy;
use crate::domain::types::OrderSide;

/// Dynamic Regime Detection Strategy
/// 
/// Adapts behavior based on market regime:
/// - Strong Trend: Looser filters, hold through pullbacks
/// - Choppy/Range-bound: Strict filters (uses Advanced strategy)
#[derive(Debug, Clone)]
pub struct DynamicRegimeStrategy {
    advanced_strategy: AdvancedTripleFilterStrategy,
    trend_divergence_threshold: f64,
}

impl DynamicRegimeStrategy {
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        sma_threshold: f64,
        trend_sma_period: usize,
        rsi_threshold: f64,
        trend_divergence_threshold: f64,
    ) -> Self {
        Self {
            advanced_strategy: AdvancedTripleFilterStrategy::new(
                fast_period,
                slow_period,
                sma_threshold,
                trend_sma_period,
                rsi_threshold,
            ),
            trend_divergence_threshold,
        }
    }
    
    fn detect_regime(&self, ctx: &AnalysisContext) -> MarketRegime {
        // Calculate divergence between fast and slow SMA as % of price
        let divergence = if ctx.price_f64 > 0.0 {
            (ctx.fast_sma - ctx.slow_sma).abs() / ctx.price_f64
        } else {
            0.0
        };
        
        if divergence > self.trend_divergence_threshold {
            MarketRegime::StrongTrend
        } else {
            MarketRegime::Choppy
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MarketRegime {
    StrongTrend,
    Choppy,
}

impl TradingStrategy for DynamicRegimeStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        let regime = self.detect_regime(ctx);
        
        match regime {
            MarketRegime::StrongTrend => {
                // In strong trends, be more permissive
                // Buy: Just need price above trend
                // Sell: Only if trend breaks (price below trend SMA)
                
                if ctx.fast_sma > ctx.slow_sma * (1.0 + 0.001) {
                    // Golden cross
                    if ctx.price_f64 > ctx.trend_sma {
                        return Some(Signal::buy(format!(
                            "Dynamic (Trend): Strong trend detected, buying above Trend SMA"
                        )));
                    }
                } else if ctx.fast_sma < ctx.slow_sma * (1.0 - 0.001) && ctx.has_position {
                    // Death cross
                    if ctx.price_f64 < ctx.trend_sma {
                        return Some(Signal::sell(format!(
                            "Dynamic (Trend): Trend broken, exiting"
                        )));
                    }
                    // Otherwise suppress sell - hold through pullback
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
    use rust_decimal_macros::dec;
    
    fn create_test_context(
        fast_sma: f64,
        slow_sma: f64,
        price: f64,
        trend_sma: f64,
    ) -> AnalysisContext {
        AnalysisContext {
            symbol: "TEST".to_string(),
            current_price: dec!(100.0),
            price_f64: price,
            fast_sma,
            slow_sma,
            trend_sma,
            rsi: 50.0,
            macd_value: 0.5,
            macd_signal: 0.3,
            macd_histogram: 0.2,
            last_macd_histogram: Some(0.1),
            atr: 1.0,
            has_position: false,
            timestamp: 0,
        }
    }
    
    #[test]
    fn test_strong_trend_buy_signal() {
        let strategy = DynamicRegimeStrategy::new(20, 60, 0.001, 200, 75.0, 0.005);
        // Large divergence = strong trend
        let ctx = create_test_context(105.0, 100.0, 110.0, 95.0);
        
        let signal = strategy.analyze(&ctx);
        
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert!(matches!(sig.side, OrderSide::Buy));
        assert!(sig.reason.contains("Dynamic (Trend)"));
    }
    
    #[test]
    fn test_strong_trend_hold_through_pullback() {
        let strategy = DynamicRegimeStrategy::new(20, 60, 0.001, 200, 75.0, 0.005);
        // Large divergence but death cross with price still above trend
        let mut ctx = create_test_context(98.0, 100.0, 102.0, 95.0);
        ctx.has_position = true;
        
        let signal = strategy.analyze(&ctx);
        
        // Should NOT sell - holding through pullback
        assert!(signal.is_none(), "Should hold through pullback in strong trend");
    }
    
    #[test]
    fn test_choppy_uses_advanced_filters() {
        let strategy = DynamicRegimeStrategy::new(20, 60, 0.001, 200, 75.0, 0.005);
        // Small divergence = choppy market
        let ctx = create_test_context(100.2, 100.0, 105.0, 95.0);
        
        let signal = strategy.analyze(&ctx);
        
        // In choppy, uses Advanced filters which would reject this
        // (MACD too weak, etc.)
        if let Some(sig) = signal {
            assert!(sig.reason.contains("Dynamic (Choppy)"));
        }
    }
}
