/// Calculate realized volatility from price returns
///
/// Returns annualized volatility (e.g., 0.15 = 15% annual volatility)
/// Uses standard deviation of log returns scaled to annual basis
pub fn calculate_realized_volatility(prices: &[f64], periods_per_year: f64) -> Option<f64> {
    if prices.len() < 2 {
        return None;
    }

    // Calculate log returns
    let mut returns = Vec::with_capacity(prices.len() - 1);
    for i in 1..prices.len() {
        if prices[i - 1] > 0.0 && prices[i] > 0.0 {
            let log_return = (prices[i] / prices[i - 1]).ln();
            returns.push(log_return);
        }
    }

    if returns.is_empty() {
        return None;
    }

    // Calculate mean return
    let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;

    // Calculate variance
    let variance: f64 = returns
        .iter()
        .map(|r| {
            let diff = r - mean;
            diff * diff
        })
        .sum::<f64>()
        / returns.len() as f64;

    // Standard deviation (volatility per period)
    let std_dev = variance.sqrt();

    // Annualize: vol_annual = vol_period * sqrt(periods_per_year)
    let annualized_vol = std_dev * periods_per_year.sqrt();

    Some(annualized_vol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_realized_volatility_calculation() {
        // Simulate prices with ~10% volatility
        let prices = vec![100.0, 102.0, 101.0, 103.0, 102.5, 104.0, 103.0, 105.0];

        // Assuming daily data, 252 trading days per year
        let vol = calculate_realized_volatility(&prices, 252.0);

        assert!(vol.is_some());
        let vol_val = vol.unwrap();

        // Should be positive and reasonable (between 0% and 100%)
        assert!(vol_val > 0.0 && vol_val < 1.0);
    }

    #[test]
    fn test_realized_volatility_insufficient_data() {
        let prices = vec![100.0];
        let vol = calculate_realized_volatility(&prices, 252.0);
        assert!(vol.is_none());
    }

    #[test]
    fn test_realized_volatility_zero_prices() {
        let prices = vec![0.0, 0.0, 0.0];
        let vol = calculate_realized_volatility(&prices, 252.0);
        assert!(vol.is_none());
    }
}
