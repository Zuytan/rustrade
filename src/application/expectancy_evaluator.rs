use crate::domain::ports::{Expectancy, ExpectancyEvaluator};
use crate::domain::market_regime::{MarketRegime, MarketRegimeType};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

pub struct MarketExpectancyEvaluator {
    min_reward_risk_ratio: f64,
}

impl MarketExpectancyEvaluator {
    pub fn new(min_reward_risk_ratio: f64) -> Self {
        Self {
            min_reward_risk_ratio,
        }
    }

    fn calculate_win_prob(&self, regime: &MarketRegime) -> f64 {
        // Base win probability based on regime and confidence
        match regime.regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                0.5 + (regime.confidence * 0.2) // Up to 70% in high-confidence trends
            }
            MarketRegimeType::Ranging => 0.45, // Lower edge in ranging
            MarketRegimeType::Volatile => 0.4,  // Harder to predict
            MarketRegimeType::Unknown => 0.3,
        }
    }
}

impl ExpectancyEvaluator for MarketExpectancyEvaluator {
    fn evaluate(
        &self,
        _symbol: &str,
        price: Decimal,
        regime: &MarketRegime,
    ) -> Expectancy {
        let price_f64 = price.to_f64().unwrap_or(0.0);
        
        // Dynamic Reward/Risk estimation
        // In reality, this should use ATR or support/resistance levels
        // Here we use a simplified model:
        // Reward = Confidence * Price * 0.05 (Target 5% move if high confidence)
        // Risk = price * 0.02 (Stop at 2%)
        
        let win_prob = self.calculate_win_prob(regime);
        let reward = if price_f64 > 0.0 { regime.confidence * price_f64 * 0.03 } else { 0.0 };
        let risk = if price_f64 > 0.0 { price_f64 * 0.015 } else { 0.0 }; // Fixed 1.5% risk for now
        
        let reward_risk_ratio = if risk > 0.0 { reward / risk } else { 0.0 };
        
        // EV = (WinProb * Reward) - (LossProb * Risk)
        let expected_value = (win_prob * reward) - ((1.0 - win_prob) * risk);
        
        Expectancy {
            reward_risk_ratio,
            win_prob,
            expected_value,
        }
    }
}
