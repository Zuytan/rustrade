use rand::Rng;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub iterations: usize,
    pub steps: usize,
    pub initial_equity: Decimal,
    pub historical_returns: Vec<Decimal>,
    pub block_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub final_equity_mean: Decimal,
    pub final_equity_median: Decimal,
    pub percentile_5: Decimal,
    pub percentile_95: Decimal,
    pub probability_of_profit: Decimal,
    pub max_drawdown_mean: f64,
}

pub struct MonteCarloEngine;

impl MonteCarloEngine {
    pub fn simulate(config: &MonteCarloConfig) -> MonteCarloResult {
        let mut rng = rand::rng();
        let mut final_equities = Vec::with_capacity(config.iterations);
        let mut max_drawdowns = Vec::with_capacity(config.iterations);
        let mut profitable_runs = 0;

        let has_returns = !config.historical_returns.is_empty();

        for _ in 0..config.iterations {
            let mut current_equity = config.initial_equity;
            let mut peak_equity = current_equity;
            let mut max_dd = 0.0;

            let mut steps_taken = 0;
            let block_size = config.block_size.max(1);
            let n_returns = if has_returns {
                config.historical_returns.len()
            } else {
                1
            };

            while steps_taken < config.steps {
                let start_idx = if has_returns {
                    rng.random_range(0..n_returns)
                } else {
                    0
                };

                let current_block_size = std::cmp::min(block_size, config.steps - steps_taken);

                for offset in 0..current_block_size {
                    let pnl_pct = if has_returns {
                        let idx = (start_idx + offset) % n_returns;
                        config.historical_returns[idx]
                    } else {
                        Decimal::ZERO
                    };

                    current_equity *= Decimal::ONE + pnl_pct;

                    if current_equity > peak_equity {
                        peak_equity = current_equity;
                    } else if peak_equity > Decimal::ZERO {
                        let dd = (peak_equity - current_equity) / peak_equity;
                        if dd.to_f64().unwrap_or(0.0) > max_dd {
                            max_dd = dd.to_f64().unwrap_or(0.0);
                        }
                    }
                }
                steps_taken += current_block_size;
            }

            final_equities.push(current_equity);
            max_drawdowns.push(max_dd);
            if current_equity > config.initial_equity {
                profitable_runs += 1;
            }
        }

        final_equities.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mean: Decimal = if config.iterations > 0 {
            let sum: Decimal = final_equities.iter().sum();
            sum / Decimal::from(config.iterations)
        } else {
            config.initial_equity
        };

        let median = if config.iterations > 0 {
            final_equities[config.iterations / 2]
        } else {
            mean
        };

        let p5 = if config.iterations > 0 {
            final_equities[config.iterations * 5 / 100]
        } else {
            mean
        };

        let p95 = if config.iterations > 0 {
            final_equities[config.iterations * 95 / 100]
        } else {
            mean
        };

        let prob_profit = if config.iterations > 0 {
            Decimal::from(profitable_runs) / Decimal::from(config.iterations)
        } else {
            Decimal::ZERO
        };

        let mean_dd: f64 = if config.iterations > 0 {
            max_drawdowns.iter().sum::<f64>() / config.iterations as f64
        } else {
            0.0
        };

        MonteCarloResult {
            final_equity_mean: mean,
            final_equity_median: median,
            percentile_5: p5,
            percentile_95: p95,
            probability_of_profit: prob_profit,
            max_drawdown_mean: mean_dd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monte_carlo_historical() {
        use rust_decimal_macros::dec;
        let config = MonteCarloConfig {
            iterations: 1000,
            steps: 50,
            initial_equity: Decimal::from(10000),
            historical_returns: vec![
                dec!(0.02),
                dec!(0.01),
                dec!(-0.015),
                dec!(0.03),
                dec!(-0.01),
            ],
            block_size: 5,
        };

        let result = MonteCarloEngine::simulate(&config);

        assert!(result.probability_of_profit > dec!(0.5));
        assert!(result.final_equity_mean > config.initial_equity);
        assert!(result.max_drawdown_mean > 0.0);
    }

    #[test]
    fn test_monte_carlo_empty_returns() {
        let config = MonteCarloConfig {
            iterations: 10,
            steps: 5,
            initial_equity: Decimal::from(10000),
            historical_returns: vec![],
            block_size: 5,
        };

        let result = MonteCarloEngine::simulate(&config);

        assert_eq!(result.probability_of_profit, Decimal::ZERO);
        assert_eq!(result.final_equity_mean, config.initial_equity);
        assert_eq!(result.max_drawdown_mean, 0.0);
    }
}
