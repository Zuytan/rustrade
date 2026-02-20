use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Shared statistics utilities for financial calculations.
pub struct Stats;

impl Stats {
    /// Calculate Sharpe Ratio.
    ///
    /// returns: daily returns
    /// annualize: if true, multiplies by sqrt(252)
    pub fn sharpe_ratio(returns: &[f64], annualize: bool) -> f64 {
        if returns.len() < 2 {
            return 0.0;
        }

        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;

        // Use sample variance (n-1)
        let variance = returns
            .iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>()
            / (returns.len() - 1) as f64;

        let std_dev = variance.sqrt();

        if std_dev > 1e-9 {
            let ratio = mean_return / std_dev;
            if annualize {
                ratio * (252.0_f64).sqrt()
            } else {
                ratio
            }
        } else {
            0.0
        }
    }

    /// Calculate Alpha and Beta using linear regression.
    ///
    /// strategy_returns: slice of strategy returns
    /// benchmark_returns: slice of benchmark returns
    /// Returns (alpha, beta, correlation)
    ///
    /// Note: Alpha returned is periodic, matching the timeframe of returns.
    pub fn alpha_beta(strategy_returns: &[f64], benchmark_returns: &[f64]) -> (f64, f64, f64) {
        let n_strat = strategy_returns.len();
        let n_bench = benchmark_returns.len();
        let n = n_strat.min(n_bench);

        if n < 2 {
            return (0.0, 0.0, 0.0);
        }

        let s = &strategy_returns[..n];
        let b = &benchmark_returns[..n];

        let mean_s: f64 = s.iter().sum::<f64>() / n as f64;
        let mean_b: f64 = b.iter().sum::<f64>() / n as f64;

        let mut cov = 0.0;
        let mut var_b = 0.0;
        let mut var_s = 0.0;

        for i in 0..n {
            let diff_s = s[i] - mean_s;
            let diff_b = b[i] - mean_b;
            cov += diff_s * diff_b;
            var_b += diff_b * diff_b;
            var_s += diff_s * diff_s;
        }

        // Use sample covariance/variance (unbiased estimator)
        cov /= (n - 1) as f64;
        var_b /= (n - 1) as f64;
        var_s /= (n - 1) as f64;

        let beta = if var_b > 1e-12 { cov / var_b } else { 0.0 };
        let alpha = mean_s - beta * mean_b;

        let correlation = if var_b > 1e-12 && var_s > 1e-12 {
            cov / (var_b.sqrt() * var_s.sqrt())
        } else {
            0.0
        };

        (alpha, beta, correlation)
    }

    /// Helper to convert Decimal prices to returns.
    pub fn calculate_returns(prices: &[Decimal]) -> Vec<f64> {
        let mut returns = Vec::new();
        for i in 1..prices.len() {
            let prev = prices[i - 1].to_f64().unwrap_or(1.0);
            let curr = prices[i].to_f64().unwrap_or(1.0);

            if prev > 0.0 {
                returns.push((curr - prev) / prev);
            }
        }
        returns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sharpe_ratio() {
        let returns = vec![0.01, 0.02, 0.01, 0.02];
        let sharpe = Stats::sharpe_ratio(&returns, false);
        assert!(sharpe > 0.0);

        let returns_zero = vec![0.01, 0.01, 0.01];
        assert_eq!(Stats::sharpe_ratio(&returns_zero, false), 0.0);
    }

    #[test]
    fn test_alpha_beta() {
        let strategy = vec![0.02, 0.04, 0.02, 0.04];
        let benchmark = vec![0.01, 0.02, 0.01, 0.02];
        let (alpha, beta, corr) = Stats::alpha_beta(&strategy, &benchmark);

        assert!(beta > 1.9 && beta < 2.1);
        assert!(alpha.abs() < 1e-6);
        assert!(corr > 0.99);
    }
}
