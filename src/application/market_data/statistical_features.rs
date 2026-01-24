//! Advanced statistical features for market analysis
//!
//! This module provides calculations for:
//! - Hurst Exponent (trend persistence detection)
//! - Skewness (distribution asymmetry)
//! - Other advanced statistical measures

/// Calculate Hurst Exponent using Rescaled Range (R/S) Analysis
///
/// The Hurst Exponent (H) measures the long-term memory of a time series:
/// - H = 0.5: Random walk (Brownian motion)
/// - H > 0.5: Trending/persistent behavior (trends continue)
/// - H < 0.5: Mean-reverting/anti-persistent (reversals likely)
///
/// # Arguments
/// * `prices` - Price series (at least 20 data points recommended)
/// * `lags` - Lag periods to analyze (e.g., &[2, 4, 8, 16])
///
/// # Returns
/// * `Some(f64)` - Hurst exponent between 0 and 1
/// * `None` - If insufficient data or calculation fails
pub fn calculate_hurst_exponent(prices: &[f64], lags: &[usize]) -> Option<f64> {
    if prices.len() < 20 || lags.is_empty() {
        return None;
    }

    // Calculate log returns
    let mut returns = Vec::with_capacity(prices.len() - 1);
    for i in 1..prices.len() {
        if prices[i - 1] > 0.0 && prices[i] > 0.0 {
            returns.push((prices[i] / prices[i - 1]).ln());
        }
    }

    if returns.is_empty() {
        return None;
    }

    // Calculate R/S for each lag
    let mut log_lags = Vec::new();
    let mut log_rs = Vec::new();

    for &lag in lags {
        if lag >= returns.len() {
            continue;
        }

        // Calculate R/S for this lag
        if let Some(rs) = calculate_rs_for_lag(&returns, lag) {
            log_lags.push((lag as f64).ln());
            log_rs.push(rs.ln());
        }
    }

    if log_lags.len() < 2 {
        return None;
    }

    // Linear regression: log(R/S) = H * log(lag) + constant
    // Slope = Hurst exponent
    let hurst = linear_regression_slope(&log_lags, &log_rs)?;

    // Clamp to valid range [0, 1]
    Some(hurst.clamp(0.0, 1.0))
}

/// Calculate Rescaled Range for a given lag
fn calculate_rs_for_lag(returns: &[f64], lag: usize) -> Option<f64> {
    let n_subseries = returns.len() / lag;
    if n_subseries == 0 {
        return None;
    }

    let mut rs_values = Vec::new();

    for i in 0..n_subseries {
        let start = i * lag;
        let end = start + lag;
        if end > returns.len() {
            break;
        }

        let subseries = &returns[start..end];

        // Calculate mean of subseries
        let mean: f64 = subseries.iter().sum::<f64>() / subseries.len() as f64;

        // Calculate cumulative deviations from mean
        let mut cumsum = 0.0;
        let mut cumulative_deviations = Vec::with_capacity(subseries.len());
        for &ret in subseries {
            cumsum += ret - mean;
            cumulative_deviations.push(cumsum);
        }

        // Range: max - min of cumulative deviations
        let max_dev = cumulative_deviations
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let min_dev = cumulative_deviations
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
        let range = max_dev - min_dev;

        // Standard deviation
        let variance: f64 =
            subseries.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / subseries.len() as f64;
        let std_dev = variance.sqrt();

        if std_dev > 0.0 {
            rs_values.push(range / std_dev);
        }
    }

    if rs_values.is_empty() {
        return None;
    }

    // Average R/S for this lag
    let avg_rs = rs_values.iter().sum::<f64>() / rs_values.len() as f64;
    Some(avg_rs)
}

/// Simple linear regression to find slope
fn linear_regression_slope(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.is_empty() {
        return None;
    }

    let n = x.len() as f64;
    let sum_x: f64 = x.iter().sum();
    let sum_y: f64 = y.iter().sum();
    let sum_xy: f64 = x.iter().zip(y.iter()).map(|(xi, yi)| xi * yi).sum();
    let sum_x2: f64 = x.iter().map(|xi| xi * xi).sum();

    let denominator = n * sum_x2 - sum_x * sum_x;
    if denominator.abs() < 1e-10 {
        return None;
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denominator;
    Some(slope)
}

/// Calculate skewness of a distribution
///
/// Skewness measures the asymmetry of the distribution:
/// - Skew = 0: Symmetric distribution
/// - Skew > 0: Right tail (positive outliers)
/// - Skew < 0: Left tail (negative outliers)
///
/// # Arguments
/// * `values` - Data points (returns, prices, etc.)
///
/// # Returns
/// * `Some(f64)` - Skewness value
/// * `None` - If insufficient data
pub fn calculate_skewness(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }

    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;

    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;

    let std_dev = variance.sqrt();
    if std_dev < 1e-10 {
        return None;
    }

    let skewness = values
        .iter()
        .map(|v| ((v - mean) / std_dev).powi(3))
        .sum::<f64>()
        / n;

    Some(skewness)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hurst_trending() {
        // Trending series: consistent upward movement
        let prices: Vec<f64> = (0..50).map(|i| 100.0 + i as f64).collect();
        let lags = vec![2, 4, 8, 16];

        let h = calculate_hurst_exponent(&prices, &lags);
        assert!(h.is_some());
        let hurst = h.unwrap();

        // Should be > 0.5 for trending
        assert!(
            hurst > 0.5,
            "Hurst for trending series should be > 0.5, got {}",
            hurst
        );
    }

    #[test]
    fn test_hurst_mean_reverting() {
        // Mean-reverting series: oscillating around mean
        let mut prices = Vec::new();
        for i in 0..50 {
            prices.push(100.0 + if i % 2 == 0 { 1.0 } else { -1.0 });
        }
        let lags = vec![2, 4, 8, 16];

        let h = calculate_hurst_exponent(&prices, &lags);
        assert!(h.is_some());
        let hurst = h.unwrap();

        // Should be < 0.5 for mean-reverting
        assert!(
            hurst < 0.5,
            "Hurst for mean-reverting series should be < 0.5, got {}",
            hurst
        );
    }

    #[test]
    fn test_hurst_insufficient_data() {
        let prices = vec![100.0, 101.0, 102.0];
        let lags = vec![2, 4];

        let h = calculate_hurst_exponent(&prices, &lags);
        assert!(h.is_none());
    }

    #[test]
    fn test_skewness_positive() {
        // Right-skewed: more small values, few large values
        let values = vec![1.0, 1.0, 1.0, 1.0, 10.0];
        let skew = calculate_skewness(&values);

        assert!(skew.is_some());
        assert!(skew.unwrap() > 0.0, "Should have positive skew");
    }

    #[test]
    fn test_skewness_negative() {
        // Left-skewed: few small values, more large values
        let values = vec![1.0, 10.0, 10.0, 10.0, 10.0];
        let skew = calculate_skewness(&values);

        assert!(skew.is_some());
        assert!(skew.unwrap() < 0.0, "Should have negative skew");
    }

    #[test]
    fn test_skewness_symmetric() {
        // Symmetric distribution
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let skew = calculate_skewness(&values);

        assert!(skew.is_some());
        // Should be close to 0
        assert!(
            skew.unwrap().abs() < 0.5,
            "Should be approximately symmetric"
        );
    }
}
