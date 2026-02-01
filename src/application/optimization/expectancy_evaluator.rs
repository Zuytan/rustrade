use crate::application::optimization::win_rate_provider::WinRateProvider;
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::ports::{Expectancy, ExpectancyEvaluator};
use rust_decimal::Decimal;
use std::sync::Arc;

pub struct MarketExpectancyEvaluator {
    #[allow(dead_code)]
    min_reward_risk_ratio: Decimal,
    win_rate_provider: Arc<dyn WinRateProvider>,
}

impl MarketExpectancyEvaluator {
    pub fn new(
        min_reward_risk_ratio: Decimal,
        win_rate_provider: Arc<dyn WinRateProvider>,
    ) -> Self {
        Self {
            min_reward_risk_ratio,
            win_rate_provider,
        }
    }

    async fn calculate_win_prob(&self, symbol: &str, regime: &MarketRegime) -> Decimal {
        // 1. Get Empirical Win Rate
        let empirical_rate_f64 = self.win_rate_provider.get_win_rate(symbol).await;
        let empirical_rate = Decimal::from_f64_retain(empirical_rate_f64).unwrap_or(dec!(0.5));

        // 2. Adjust based on Regime Confidence
        use rust_decimal_macros::dec;
        let regime_modifier = match regime.regime_type {
            MarketRegimeType::TrendingUp | MarketRegimeType::TrendingDown => {
                dec!(0.05) * regime.confidence // +0% to +5% boost
            }
            MarketRegimeType::Ranging => Decimal::ZERO,
            MarketRegimeType::Volatile => dec!(-0.05),
            MarketRegimeType::Unknown => dec!(-0.10),
        };

        // Clamp between 0.1 and 0.9
        (empirical_rate + regime_modifier).clamp(dec!(0.1), dec!(0.9))
    }
}

#[async_trait::async_trait]
impl ExpectancyEvaluator for MarketExpectancyEvaluator {
    async fn evaluate(&self, symbol: &str, price: Decimal, regime: &MarketRegime) -> Expectancy {
        // Dynamic Reward/Risk estimation
        use rust_decimal_macros::dec;

        let win_prob = self.calculate_win_prob(symbol, regime).await;

        let reward = if price > Decimal::ZERO {
            regime.confidence * price * dec!(0.03)
        } else {
            Decimal::ZERO
        };
        let risk = if price > Decimal::ZERO {
            price * dec!(0.015)
        } else {
            Decimal::ZERO
        }; // Fixed 1.5% risk for now

        let reward_risk_ratio = if risk > Decimal::ZERO {
            reward / risk
        } else {
            Decimal::ZERO
        };

        // EV = (WinProb * Reward) - (LossProb * Risk)
        let expected_value = (win_prob * reward) - ((Decimal::ONE - win_prob) * risk);

        Expectancy {
            reward_risk_ratio,
            win_prob,
            expected_value,
        }
    }
}
