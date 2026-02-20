use rust_decimal::Decimal;

/// Shared statistics utilities for financial calculations.
pub struct Stats;

impl Stats {
    /// Calculate Sharpe Ratio using Decimal.
    ///
    /// returns: daily returns
    /// annualize: if true, multiplies by sqrt(252)
    pub fn sharpe_ratio(returns: &[Decimal], annualize: bool) -> Decimal {
        if returns.len() < 2 {
            return Decimal::ZERO;
        }

        let n = Decimal::from(returns.len());
        let sum: Decimal = returns.iter().sum();
        let mean_return = sum / n;

        // Use sample variance (n-1)
        let mut variance_sum = Decimal::ZERO;
        for r in returns {
            let diff = r - mean_return;
            variance_sum += diff * diff;
        }

        let n_minus_1 = Decimal::from(returns.len() - 1);
        let variance = variance_sum / n_minus_1;

        let std_dev_f64 = rust_decimal::prelude::ToPrimitive::to_f64(&variance)
            .unwrap_or(0.0)
            .sqrt();
        let std_dev = Decimal::from_f64_retain(std_dev_f64).unwrap_or(Decimal::ZERO);

        if std_dev > rust_decimal_macros::dec!(1e-9) {
            let ratio = mean_return / std_dev;
            if annualize {
                let sqrt_252 =
                    Decimal::from_f64_retain(15.874507866387544).unwrap_or(Decimal::ZERO);
                ratio * sqrt_252
            } else {
                ratio
            }
        } else {
            Decimal::ZERO
        }
    }

    /// Calculate Alpha and Beta using linear regression.
    pub fn alpha_beta(
        strategy_returns: &[Decimal],
        benchmark_returns: &[Decimal],
    ) -> (Decimal, Decimal, Decimal) {
        let n_strat = strategy_returns.len();
        let n_bench = benchmark_returns.len();
        let n = n_strat.min(n_bench);

        if n < 2 {
            return (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
        }

        let s = &strategy_returns[..n];
        let b = &benchmark_returns[..n];

        let n_dec = Decimal::from(n);
        let mean_s: Decimal = s.iter().sum::<Decimal>() / n_dec;
        let mean_b: Decimal = b.iter().sum::<Decimal>() / n_dec;

        let mut cov = Decimal::ZERO;
        let mut var_b = Decimal::ZERO;
        let mut var_s = Decimal::ZERO;

        for i in 0..n {
            let diff_s = s[i] - mean_s;
            let diff_b = b[i] - mean_b;
            cov += diff_s * diff_b;
            var_b += diff_b * diff_b;
            var_s += diff_s * diff_s;
        }

        // Use sample covariance/variance (unbiased estimator)
        let n_minus_1 = Decimal::from(n - 1);
        cov /= n_minus_1;
        var_b /= n_minus_1;
        var_s /= n_minus_1;

        let beta = if var_b > rust_decimal_macros::dec!(1e-12) {
            cov / var_b
        } else {
            Decimal::ZERO
        };
        let alpha = mean_s - beta * mean_b;

        let correlation = if var_b > rust_decimal_macros::dec!(1e-12)
            && var_s > rust_decimal_macros::dec!(1e-12)
        {
            let var_b_f64 = rust_decimal::prelude::ToPrimitive::to_f64(&var_b)
                .unwrap_or(0.0)
                .sqrt();
            let var_s_f64 = rust_decimal::prelude::ToPrimitive::to_f64(&var_s)
                .unwrap_or(0.0)
                .sqrt();
            let denom = Decimal::from_f64_retain(var_b_f64 * var_s_f64).unwrap_or(Decimal::ONE);
            if denom > Decimal::ZERO {
                cov / denom
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ZERO
        };

        (alpha, beta, correlation)
    }

    pub fn calculate_returns(prices: &[Decimal]) -> Vec<Decimal> {
        let mut returns = Vec::new();
        for i in 1..prices.len() {
            let prev = prices[i - 1];
            let curr = prices[i];

            if prev > Decimal::ZERO {
                returns.push((curr - prev) / prev);
            }
        }
        returns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_sharpe_ratio() {
        let returns = vec![dec!(0.01), dec!(0.02), dec!(0.01), dec!(0.02)];
        let sharpe = Stats::sharpe_ratio(&returns, false);
        assert!(sharpe > Decimal::ZERO);

        let returns_zero = vec![dec!(0.01), dec!(0.01), dec!(0.01)];
        assert_eq!(Stats::sharpe_ratio(&returns_zero, false), Decimal::ZERO);
    }

    #[test]
    fn test_alpha_beta() {
        let strategy = vec![dec!(0.02), dec!(0.04), dec!(0.02), dec!(0.04)];
        let benchmark = vec![dec!(0.01), dec!(0.02), dec!(0.01), dec!(0.02)];
        let (alpha, beta, corr) = Stats::alpha_beta(&strategy, &benchmark);

        assert!(beta > dec!(1.9) && beta < dec!(2.1));
        assert!(alpha.abs() < dec!(1e-6));
        assert!(corr > dec!(0.99));
    }
}
