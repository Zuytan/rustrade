use crate::domain::performance_snapshot::PerformanceSnapshot;
use crate::domain::reoptimization_trigger::TriggerReason;

/// Configuration thresholds for performance evaluation
pub struct EvaluationThresholds {
    pub min_sharpe: f64,
    pub max_drawdown: f64,
    pub min_win_rate: f64,
}

impl Default for EvaluationThresholds {
    fn default() -> Self {
        Self {
            min_sharpe: 0.5,
            max_drawdown: 0.15,
            min_win_rate: 0.40,
        }
    }
}

/// Service to evaluate if performance warrants re-optimization
pub struct PerformanceEvaluator {
    thresholds: EvaluationThresholds,
}

impl PerformanceEvaluator {
    pub fn new(thresholds: EvaluationThresholds) -> Self {
        Self { thresholds }
    }

    /// Check if current metrics trigger re-optimization
    pub fn evaluate(&self, snapshot: &PerformanceSnapshot) -> Option<TriggerReason> {
        // 1. Check Drawdown
        if snapshot.drawdown_pct > self.thresholds.max_drawdown {
            return Some(TriggerReason::DrawdownLimit);
        }

        // 2. Check Sharpe Ratio
        // Only evaluate if we have meaningful data (e.g. non-zero)
        if snapshot.sharpe_rolling_30d != 0.0 && snapshot.sharpe_rolling_30d < self.thresholds.min_sharpe {
            return Some(TriggerReason::PoorPerformance);
        }

        // 3. Check Win Rate
        if snapshot.win_rate_rolling_30d != 0.0 && snapshot.win_rate_rolling_30d < self.thresholds.min_win_rate {
            return Some(TriggerReason::PoorPerformance);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::market_regime::MarketRegimeType;
    use rust_decimal::Decimal;

    #[test]
    fn test_drawdown_trigger() {
        let thresholds = EvaluationThresholds {
            max_drawdown: 0.10, // 10%
            ..Default::default()
        };
        let evaluator = PerformanceEvaluator::new(thresholds);

        let snapshot = PerformanceSnapshot::new(
            "TEST".to_string(),
            Decimal::new(10000, 0),
            0.15, // 15% drawdown
            1.0,
            0.5,
            MarketRegimeType::TrendingUp,
        );

        assert_eq!(evaluator.evaluate(&snapshot), Some(TriggerReason::DrawdownLimit));
    }

    #[test]
    fn test_sharpe_trigger() {
        let thresholds = EvaluationThresholds {
            min_sharpe: 1.0, 
            ..Default::default()
        };
        let evaluator = PerformanceEvaluator::new(thresholds);

        let snapshot = PerformanceSnapshot::new(
            "TEST".to_string(),
            Decimal::new(10000, 0),
            0.05,
            0.8, // Low Sharpe
            0.5,
            MarketRegimeType::TrendingUp,
        );

        assert_eq!(evaluator.evaluate(&snapshot), Some(TriggerReason::PoorPerformance));
    }
}
