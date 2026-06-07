use rust_decimal_macros::dec;
use rustrade::application::optimization::optimizer::ParameterGrid;
use rustrade::domain::risk::risk_appetite::RiskProfile;

/// Default parameter grid for crypto (genetic algo bounds). More responsive SMAs, shorter cooldowns (24/7).
pub fn default_grid_crypto() -> ParameterGrid {
    get_grid_for_profile(
        RiskProfile::Balanced,
        rustrade::domain::risk::optimal_parameters::AssetType::Crypto,
    )
}

/// Grid for Ensemble strategy (StatMomentum + ZScoreMR + SMC). Sets modern param bounds for genetic search.
/// Legacy params are kept narrow; the 8 modern params drive the Ensemble's signal generation.
pub fn grid_for_ensemble_crypto() -> ParameterGrid {
    ParameterGrid {
        fast_sma: vec![10, 20],
        slow_sma: vec![50, 80],
        rsi_threshold: vec![dec!(60.0), dec!(70.0)],
        trend_divergence_threshold: vec![dec!(0.005), dec!(0.01)],
        trailing_stop_atr_multiplier: vec![dec!(2.0), dec!(3.0), dec!(4.0)],
        order_cooldown_seconds: vec![0, 120, 300],
        stat_momentum_lookback: Some(vec![8, 10, 14, 20]),
        stat_momentum_threshold: Some(vec![dec!(1.0), dec!(1.5), dec!(2.0), dec!(2.5)]),
        zscore_lookback: Some(vec![15, 20, 25, 30]),
        zscore_entry_threshold: Some(vec![dec!(-2.5), dec!(-2.0), dec!(-1.5)]),
        zscore_exit_threshold: Some(vec![dec!(-0.5), dec!(0.0), dec!(0.5)]),
        ofi_threshold: Some(vec![dec!(0.2), dec!(0.3), dec!(0.4)]),
        smc_ob_lookback: Some(vec![15, 20, 25, 30]),
        smc_min_fvg_size_pct: Some(vec![dec!(0.001), dec!(0.005), dec!(0.01), dec!(0.02)]),
    }
}

/// Returns a parameter grid tailored for a specific risk profile and asset type.
/// Crypto uses more responsive SMAs and shorter cooldowns (24/7 volatile markets).
pub fn get_grid_for_profile(
    profile: RiskProfile,
    asset: rustrade::domain::risk::optimal_parameters::AssetType,
) -> ParameterGrid {
    let is_crypto = asset == rustrade::domain::risk::optimal_parameters::AssetType::Crypto;
    match profile {
        RiskProfile::Conservative => ParameterGrid {
            fast_sma: vec![10, 15, 20],
            slow_sma: vec![50, 60, 80, 100],
            rsi_threshold: vec![dec!(54.0), dec!(58.0), dec!(62.0), dec!(66.0)],
            trend_divergence_threshold: vec![dec!(0.002), dec!(0.003), dec!(0.005)],
            trailing_stop_atr_multiplier: vec![dec!(1.5), dec!(2.0), dec!(2.5), dec!(3.0)],
            order_cooldown_seconds: if is_crypto {
                vec![60, 180, 420]
            } else {
                vec![300, 600, 900]
            },
            ..Default::default()
        },
        RiskProfile::Balanced => ParameterGrid {
            fast_sma: if is_crypto {
                vec![8, 14, 20, 26]
            } else {
                vec![12, 18, 22, 28]
            },
            slow_sma: vec![50, 65, 85, 110],
            rsi_threshold: vec![dec!(58.0), dec!(63.0), dec!(68.0), dec!(72.0)],
            trend_divergence_threshold: vec![dec!(0.003), dec!(0.005), dec!(0.007), dec!(0.01)],
            trailing_stop_atr_multiplier: vec![dec!(2.0), dec!(2.75), dec!(3.5), dec!(4.5)],
            order_cooldown_seconds: if is_crypto {
                vec![0, 120, 300]
            } else {
                vec![0, 180, 420, 600]
            },
            ..Default::default()
        },
        RiskProfile::Aggressive => ParameterGrid {
            fast_sma: if is_crypto {
                vec![10, 18, 26]
            } else {
                vec![18, 24, 30]
            },
            slow_sma: vec![55, 70, 90, 120],
            rsi_threshold: vec![dec!(62.0), dec!(67.0), dec!(72.0), dec!(76.0)],
            trend_divergence_threshold: vec![dec!(0.004), dec!(0.006), dec!(0.009), dec!(0.012)],
            trailing_stop_atr_multiplier: vec![dec!(3.0), dec!(4.0), dec!(5.0), dec!(6.0)],
            order_cooldown_seconds: if is_crypto {
                vec![0, 60]
            } else {
                vec![0, 60, 180]
            },
            ..Default::default()
        },
    }
}

/// Calculates the number of parameter combinations in a grid.
pub fn calculate_grid_combinations(grid: &ParameterGrid) -> usize {
    let mut count = 0;
    for fast in &grid.fast_sma {
        for slow in &grid.slow_sma {
            if fast >= slow {
                continue;
            }
            count += grid.rsi_threshold.len()
                * grid.trend_divergence_threshold.len()
                * grid.trailing_stop_atr_multiplier.len()
                * grid.order_cooldown_seconds.len();
        }
    }
    count
}
