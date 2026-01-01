use crate::application::optimization::win_rate_provider::WinRateProvider;
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::ports::{Expectancy, ExpectancyEvaluator};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::sync::Arc;

pub struct MarketExpectancyEvaluator {
    #[allow(dead_code)]
    min_reward_risk_ratio: f64,
    win_rate_provider: Arc<dyn WinRateProvider>,
}

impl MarketExpectancyEvaluator {
    pub fn new(min_reward_risk_ratio: f64, win_rate_provider: Arc<dyn WinRateProvider>) -> Self {
        Self {
            min_reward_risk_ratio,
            win_rate_provider,
        }
    }

    async fn calculate_win_prob(&self, symbol: &str, regime: &MarketRegime) -> f64 {
        // 1. Get Empirical Win Rate
        let empirical_rate = self.win_rate_provider.get_win_rate(symbol).await;
        
        // 2. Adjust based on Regime Confidence (simple bayesian-like update or weighted avg)
        // If High Confidence Trend, boost win rate.
        // If Volatile/Unknown, discount win rate.
        
        let regime_modifier = match regime.regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                0.05 * regime.confidence // +0% to +5% boost
            }
            MarketRegimeType::Ranging => 0.0,
            MarketRegimeType::Volatile => -0.05,
            MarketRegimeType::Unknown => -0.10,
        };

        // Clamp between 0.1 and 0.9
        (empirical_rate + regime_modifier).clamp(0.1, 0.9)
    }
}

#[async_trait::async_trait]
impl ExpectancyEvaluator for MarketExpectancyEvaluator {
    async fn evaluate(&self, symbol: &str, price: Decimal, regime: &MarketRegime) -> Expectancy {
        let price_f64 = price.to_f64().unwrap_or(0.0);


        // Dynamic Reward/Risk estimation
        // In reality, this should use ATR or support/resistance levels
        // Here we use a simplified model:
        // Reward = Confidence * Price * 0.05 (Target 5% move if high confidence)
        // Risk = price * 0.02 (Stop at 2%)

        // Risk = price * 0.02 (Stop at 2%)

        let win_prob = self.calculate_win_prob(symbol, regime).await;

        let reward = if price_f64 > 0.0 {
            regime.confidence * price_f64 * 0.03
        } else {
            0.0
        };
        let risk = if price_f64 > 0.0 {
            price_f64 * 0.015
        } else {
            0.0
        }; // Fixed 1.5% risk for now

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
