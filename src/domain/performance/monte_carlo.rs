use rand::Rng;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub iterations: usize,
    pub steps: usize,
    pub initial_equity: Decimal,
    pub win_rate: f64,
    pub avg_win_pct: f64,
    pub avg_loss_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub final_equity_mean: Decimal,
    pub final_equity_median: Decimal,
    pub percentile_5: Decimal,
    pub percentile_95: Decimal,
    pub probability_of_profit: f64,
    pub max_drawdown_mean: f64,
}

pub struct MonteCarloEngine;

impl MonteCarloEngine {
    pub fn simulate(config: &MonteCarloConfig) -> MonteCarloResult {
        let mut rng = rand::rng();
        let mut final_equities = Vec::with_capacity(config.iterations);
        let mut max_drawdowns = Vec::with_capacity(config.iterations);
        let mut profitable_runs = 0;

        for _ in 0..config.iterations {
            let mut current_equity = config.initial_equity.to_f64().unwrap_or(0.0);
            let mut peak_equity = current_equity;
            let mut max_dd = 0.0;

            for _ in 0..config.steps {
                let is_win = rng.random_bool(config.win_rate);
                let pnl_pct = if is_win {
                    config.avg_win_pct
                } else {
                    -config.avg_loss_pct
                };

                current_equity *= 1.0 + pnl_pct;

                if current_equity > peak_equity {
                    peak_equity = current_equity;
                } else {
                    let dd = (peak_equity - current_equity) / peak_equity;
                    if dd > max_dd {
                        max_dd = dd;
                    }
                }
            }

            final_equities.push(current_equity);
            max_drawdowns.push(max_dd);
            if current_equity > config.initial_equity.to_f64().unwrap_or(0.0) {
                profitable_runs += 1;
            }
        }

        final_equities.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mean: f64 = final_equities.iter().sum::<f64>() / config.iterations as f64;
        let median = final_equities[config.iterations / 2];
        let p5 = final_equities[config.iterations * 5 / 100];
        let p95 = final_equities[config.iterations * 95 / 100];
        let prob_profit = profitable_runs as f64 / config.iterations as f64;
        let mean_dd: f64 = max_drawdowns.iter().sum::<f64>() / config.iterations as f64;

        MonteCarloResult {
            final_equity_mean: Decimal::from_f64_retain(mean).unwrap_or_default(),
            final_equity_median: Decimal::from_f64_retain(median).unwrap_or_default(),
            percentile_5: Decimal::from_f64_retain(p5).unwrap_or_default(),
            percentile_95: Decimal::from_f64_retain(p95).unwrap_or_default(),
            probability_of_profit: prob_profit,
            max_drawdown_mean: mean_dd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monte_carlo_basic() {
        let config = MonteCarloConfig {
            iterations: 1000,
            steps: 50,
            initial_equity: Decimal::from(10000),
            win_rate: 0.6,
            avg_win_pct: 0.02,
            avg_loss_pct: 0.015,
        };

        let result = MonteCarloEngine::simulate(&config);

        assert!(result.probability_of_profit > 0.5);
        assert!(result.final_equity_mean > config.initial_equity);
        assert!(result.max_drawdown_mean >= 0.0);
    }
}
