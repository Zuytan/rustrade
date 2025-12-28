use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Risk profile classification based on appetite score
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskProfile {
    /// Conservative approach: Capital preservation (scores 1-3)
    Conservative,
    /// Balanced approach: Moderate risk/return (scores 4-7)
    Balanced,
    /// Aggressive approach: High risk/return (scores 8-10)
    Aggressive,
}

/// Value object representing user's risk appetite on a scale of 1-10
///
/// This domain object encapsulates the risk tolerance and provides
/// calculated trading parameters based on the risk profile.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RiskAppetite {
    score: u8,
}

impl RiskAppetite {
    /// Creates a new RiskAppetite with validation
    ///
    /// # Arguments
    /// * `score` - Risk appetite score between 1 and 10 (inclusive)
    ///
    /// # Returns
    /// * `Ok(RiskAppetite)` if score is valid
    /// * `Err` if score is outside valid range
    pub fn new(score: u8) -> Result<Self> {
        if !(1..=9).contains(&score) {
            bail!(
                "Risk appetite score must be between 1 and 9, got: {}",
                score
            );
        }
        Ok(Self { score })
    }

    /// Returns the raw score value
    pub fn score(&self) -> u8 {
        self.score
    }

    /// Classifies the risk appetite into a profile
    pub fn profile(&self) -> RiskProfile {
        match self.score {
            1..=3 => RiskProfile::Conservative,
            4..=6 => RiskProfile::Balanced, // Center 5
            7..=9 => RiskProfile::Aggressive,
            _ => unreachable!("Score validated in constructor"),
        }
    }

    /// Calculates the risk per trade percentage based on appetite
    ///
    /// Returns a value between 0.005 (0.5%) for score 1 and 0.03 (3%) for score 10
    /// Uses continuous linear interpolation for smooth progression
    pub fn calculate_risk_per_trade_percent(&self) -> f64 {
        Self::interpolate(self.score, 1, 9, 0.005, 0.03)
    }

    /// Calculates the trailing stop ATR multiplier based on appetite
    ///
    /// Returns a value between 2.0 (tight stops) for score 1 and 5.0 (loose stops) for score 9
    /// Uses continuous linear interpolation for smooth progression
    pub fn calculate_trailing_stop_multiplier(&self) -> f64 {
        Self::interpolate(self.score, 1, 9, 2.0, 5.0)
    }

    /// Calculates the RSI threshold for buy signals based on appetite
    ///
    /// Returns a value between 30 (wait for oversold) for score 1
    /// and 75 (follow momentum) for score 9
    /// Uses continuous linear interpolation for smooth progression
    pub fn calculate_rsi_threshold(&self) -> f64 {
        Self::interpolate(self.score, 1, 9, 30.0, 75.0)
    }

    /// Calculates the maximum position size as percentage of portfolio
    ///
    /// Returns a value between 0.05 (5%) for score 1 and 0.30 (30%) for score 9
    /// Uses continuous linear interpolation for smooth progression
    pub fn calculate_max_position_size_pct(&self) -> f64 {
        Self::interpolate(self.score, 1, 9, 0.05, 0.30)
    }

    /// Linear interpolation helper
    ///
    /// Maps a score within [score_min, score_max] to a value within [value_min, value_max]
    fn interpolate(score: u8, score_min: u8, score_max: u8, value_min: f64, value_max: f64) -> f64 {
        let score_range = (score_max - score_min) as f64;
        let score_offset = (score - score_min) as f64;
        let ratio = score_offset / score_range;
        value_min + ratio * (value_max - value_min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_appetite_score_validation_success() {
        // Valid scores should succeed
        for score in 1..=9 {
            let result = RiskAppetite::new(score);
            assert!(
                result.is_ok(),
                "Score {} should be valid, got error: {:?}",
                score,
                result.err()
            );
            assert_eq!(result.unwrap().score(), score);
        }
    }

    #[test]
    fn test_risk_appetite_score_validation_failure() {
        // Invalid scores should fail
        let invalid_scores = [0, 10, 15, 100, 255];
        for score in invalid_scores {
            let result = RiskAppetite::new(score);
            assert!(
                result.is_err(),
                "Score {} should be invalid but passed validation",
                score
            );
        }
    }

    #[test]
    fn test_risk_profile_classification() {
        // Conservative: 1-3
        assert_eq!(
            RiskAppetite::new(1).unwrap().profile(),
            RiskProfile::Conservative
        );
        assert_eq!(
            RiskAppetite::new(2).unwrap().profile(),
            RiskProfile::Conservative
        );
        assert_eq!(
            RiskAppetite::new(3).unwrap().profile(),
            RiskProfile::Conservative
        );

        // Balanced: 4-6
        assert_eq!(
            RiskAppetite::new(4).unwrap().profile(),
            RiskProfile::Balanced
        );
        assert_eq!(
            RiskAppetite::new(5).unwrap().profile(),
            RiskProfile::Balanced
        );
        assert_eq!(
            RiskAppetite::new(6).unwrap().profile(),
            RiskProfile::Balanced
        );

        // Aggressive: 7-9
        assert_eq!(
            RiskAppetite::new(7).unwrap().profile(),
            RiskProfile::Aggressive
        );
        assert_eq!(
            RiskAppetite::new(8).unwrap().profile(),
            RiskProfile::Aggressive
        );
        assert_eq!(
            RiskAppetite::new(9).unwrap().profile(),
            RiskProfile::Aggressive
        );
    }

    #[test]
    fn test_conservative_profile_parameters() {
        let risk = RiskAppetite::new(2).unwrap();

        // With continuous interpolation, score 2 should be:
        // - 1/9 of the way from min to max (score 2 out of 1-10 range)
        let risk_per_trade = risk.calculate_risk_per_trade_percent();
        assert!(
            (0.005..=0.015).contains(&risk_per_trade),
            "Score 2 risk per trade should be early in range, got {}",
            risk_per_trade
        );

        let trailing_stop = risk.calculate_trailing_stop_multiplier();
        assert!(
            (2.0..=3.0).contains(&trailing_stop),
            "Score 2 trailing stop should be early in range, got {}",
            trailing_stop
        );

        let rsi_threshold = risk.calculate_rsi_threshold();
        assert!(
            (30.0..=40.0).contains(&rsi_threshold),
            "Score 2 RSI threshold should be low, got {}",
            rsi_threshold
        );

        let max_position = risk.calculate_max_position_size_pct();
        assert!(
            (0.05..=0.12).contains(&max_position),
            "Score 2 max position should be small, got {}",
            max_position
        );
    }

    #[test]
    fn test_balanced_profile_parameters() {
        let risk = RiskAppetite::new(5).unwrap();

        // Score 5 is EXACTLY mid-range (4/8 through the scale)
        let risk_per_trade = risk.calculate_risk_per_trade_percent();
        // 0.005 + 0.5 * (0.03 - 0.005) = 0.0175
        assert!((risk_per_trade - 0.0175).abs() < 1e-10);

        let trailing_stop = risk.calculate_trailing_stop_multiplier();
        // 2.0 + 0.5 * (5.0 - 2.0) = 3.5
        assert!((trailing_stop - 3.5).abs() < 1e-10);

        let rsi_threshold = risk.calculate_rsi_threshold();
        // 30.0 + 0.5 * (75.0 - 30.0) = 52.5
        assert!((rsi_threshold - 52.5).abs() < 1e-10);

        let max_position = risk.calculate_max_position_size_pct();
        // 0.05 + 0.5 * (0.30 - 0.05) = 0.175
        assert!((max_position - 0.175).abs() < 1e-10);
    }

    #[test]
    fn test_aggressive_profile_parameters() {
        let risk = RiskAppetite::new(9).unwrap();

        // Score 9 should be near the high end (8/9 through the scale)
        let risk_per_trade = risk.calculate_risk_per_trade_percent();
        assert!(
            (0.025..=0.03).contains(&risk_per_trade),
            "Score 9 risk per trade should be high, got {}",
            risk_per_trade
        );

        let trailing_stop = risk.calculate_trailing_stop_multiplier();
        assert!(
            (4.5..=5.0).contains(&trailing_stop),
            "Score 9 trailing stop should be high, got {}",
            trailing_stop
        );

        let rsi_threshold = risk.calculate_rsi_threshold();
        assert!(
            (70.0..=75.0).contains(&rsi_threshold),
            "Score 9 RSI threshold should be high, got {}",
            rsi_threshold
        );

        let max_position = risk.calculate_max_position_size_pct();
        assert!(
            (0.27..=0.30).contains(&max_position),
            "Score 9 max position should be high, got {}",
            max_position
        );
    }

    #[test]
    fn test_parameter_interpolation() {
        // Test that parameters smoothly interpolate within each profile

        // Conservative range (1-3)
        let risk1 = RiskAppetite::new(1).unwrap();
        let risk3 = RiskAppetite::new(3).unwrap();
        assert!(
            risk1.calculate_risk_per_trade_percent() < risk3.calculate_risk_per_trade_percent()
        );
        assert!(
            risk1.calculate_trailing_stop_multiplier() < risk3.calculate_trailing_stop_multiplier()
        );

        // Balanced range (4-6)
        let risk4 = RiskAppetite::new(4).unwrap();
        let risk6 = RiskAppetite::new(6).unwrap();
        assert!(
            risk4.calculate_risk_per_trade_percent() < risk6.calculate_risk_per_trade_percent()
        );
        assert!(risk4.calculate_rsi_threshold() < risk6.calculate_rsi_threshold());

        // Aggressive range (7-9)
        let risk7 = RiskAppetite::new(7).unwrap();
        let risk9 = RiskAppetite::new(9).unwrap();
        assert!(risk7.calculate_max_position_size_pct() < risk9.calculate_max_position_size_pct());
        assert!(
            risk7.calculate_trailing_stop_multiplier()
                < risk9.calculate_trailing_stop_multiplier()
        );
    }

    #[test]
    fn test_score_7_vs_9_difference() {
        // Verify that there is a meaningful difference between score 7 and 9
        let risk7 = RiskAppetite::new(7).unwrap();
        let risk9 = RiskAppetite::new(9).unwrap();

        // Calculate percentage differences
        let risk_trade_diff = (risk9.calculate_risk_per_trade_percent()
            - risk7.calculate_risk_per_trade_percent())
            / risk7.calculate_risk_per_trade_percent();

        let trailing_stop_diff = (risk9.calculate_trailing_stop_multiplier()
            - risk7.calculate_trailing_stop_multiplier())
            / risk7.calculate_trailing_stop_multiplier();

        let position_size_diff = (risk9.calculate_max_position_size_pct()
            - risk7.calculate_max_position_size_pct())
            / risk7.calculate_max_position_size_pct();

        // With continuous interpolation, score 7 to 9 is 2/8 = 25% of the range
        // So we expect meaningful differences
        assert!(risk_trade_diff > 0.15);
        assert!(trailing_stop_diff > 0.10);
        assert!(position_size_diff > 0.15);
    }

    #[test]
    fn test_monotonic_increase_across_profiles() {
        // Verify that parameters increase as score increases across all profiles
        let scores_to_test = [1, 3, 5, 7, 9];
        let risks: Vec<_> = scores_to_test
            .iter()
            .map(|&s| RiskAppetite::new(s).unwrap())
            .collect();

        for i in 0..risks.len() - 1 {
            let current = &risks[i];
            let next = &risks[i + 1];

            assert!(
                current.calculate_risk_per_trade_percent()
                    <= next.calculate_risk_per_trade_percent(),
                "Risk per trade should increase with score"
            );
            assert!(
                current.calculate_trailing_stop_multiplier()
                    <= next.calculate_trailing_stop_multiplier(),
                "Trailing stop multiplier should increase with score"
            );
            assert!(
                current.calculate_max_position_size_pct() <= next.calculate_max_position_size_pct(),
                "Max position size should increase with score"
            );
        }
    }
}
