use anyhow::{Result, bail};
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

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

impl RiskAppetite {
    /// Creates a new RiskAppetite with validation
    ///
    /// # Arguments
    /// * `score` - Risk appetite score between 1 and 9 (inclusive)
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
    /// Returns a value between 0.003 (0.3%) for score 1 and 0.28 (28%) for score 9.
    /// Wide spread so Risk-2 vs Risk-8 shows clearly different position sizes.
    pub fn calculate_risk_per_trade_percent(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(0.003), dec!(0.28))
    }

    /// Calculates the trailing stop ATR multiplier based on appetite
    ///
    /// Returns a value between 1.2 (very tight) for score 1 and 10.0 (wide) for score 9.
    pub fn calculate_trailing_stop_multiplier(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(1.2), dec!(10.0))
    }

    /// Calculates the RSI threshold for buy signals based on appetite
    ///
    /// Returns a value between 55 (wait for oversold) for score 1
    /// and 85 (follow momentum) for score 9
    /// Uses continuous linear interpolation for smooth progression
    pub fn calculate_rsi_threshold(&self) -> Decimal {
        // Conservative still gets a reasonable ceiling (55) vs Aggressive (85)
        Self::interpolate(self.score, 1, 9, dec!(55.0), dec!(85.0))
    }

    /// Calculates the maximum position size as percentage of portfolio
    ///
    /// Returns a value between 0.02 (2%) for score 1 and 1.00 (100%) for score 9.
    /// Conservative stays very small, aggressive can go full size.
    pub fn calculate_max_position_size_pct(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(0.02), dec!(1.00))
    }

    /// Calculate minimum profit-to-cost ratio threshold
    /// Conservative traders require higher profit margins
    /// Aggressive traders accept lower margins for more opportunities
    pub fn calculate_min_profit_ratio(&self) -> Decimal {
        // Inverse relationship: higher risk appetite = lower profit requirement
        // Score 1 (conservative): 2.0 (strict but achievable)
        // Score 9 (aggressive): 0.5 (permissive)
        Self::interpolate(self.score, 1, 9, dec!(2.0), dec!(0.5))
    }

    /// Determine if MACD histogram must be rising for buy signals
    /// Only extreme conservative traders (score 1-2) require rising momentum
    /// Others accept positive momentum even if not rising
    pub fn requires_macd_rising(&self) -> bool {
        // Score <= 2: require rising (very conservative)
        // Score >= 3: just positive is OK
        self.score <= 2
    }

    /// Calculate trend filter tolerance percentage
    /// Conservative traders require strict trend alignment
    /// Aggressive traders allow more deviation from trend
    pub fn calculate_trend_tolerance_pct(&self) -> Decimal {
        // Score 1 (conservative): 0% tolerance (price must be > trend_sma)
        // Score 9 (aggressive): 15% tolerance (price > trend_sma * 0.85)
        Self::interpolate(self.score, 1, 9, Decimal::ZERO, dec!(0.15))
    }

    /// Calculate minimum MACD histogram threshold for buy signals
    /// Conservative traders require clearly positive momentum
    /// Aggressive traders accept near-neutral or slightly negative
    pub fn calculate_macd_min_threshold(&self) -> Decimal {
        // Score 1 (conservative): +0.02 (clearly positive)
        // Score 9 (aggressive): -0.05 (negative OK)
        Self::interpolate(self.score, 1, 9, dec!(0.02), dec!(-0.05))
    }

    /// Calculate profit target multiplier (Risk/Reward expectation)
    /// Conservative traders target modest gains (1.5x ATR)
    /// Aggressive traders target larger swings (3.0x ATR)
    pub fn calculate_profit_target_multiplier(&self) -> Decimal {
        // Score 1: 1.5x ATR
        // Score 9: 10.0x ATR
        Self::interpolate(self.score, 1, 9, dec!(1.5), dec!(10.0))
    }

    /// Calculate signal sensitivity factor for entry signal thresholds
    /// Conservative: lower multiplier = stricter effective threshold = fewer signals.
    /// Aggressive: 1.0 = standard thresholds = more signals.
    pub fn calculate_signal_sensitivity_factor(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(0.4), dec!(1.0))
    }

    /// Number of confirmation bars required before entering (1 = fast, 3 = cautious).
    /// Conservative requires more confirmation => fewer trades; aggressive enters sooner.
    pub fn calculate_signal_confirmation_bars(&self) -> usize {
        match self.score {
            1..=2 => 3,
            3..=4 => 2,
            _ => 1,
        }
    }

    /// Calculate Maximum Daily Loss Percentage
    /// Conservative: 1% (0.01)
    /// Aggressive: 5% (0.05)
    pub fn calculate_max_daily_loss_pct(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(0.01), dec!(0.05))
    }

    /// Calculate Maximum Drawdown Percentage
    /// Conservative: 3% (0.03)
    /// Aggressive: 15% (0.15)
    pub fn calculate_max_drawdown_pct(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(0.03), dec!(0.15))
    }

    /// Maximum loss per trade (negative decimal, e.g. -0.02 = -2%)
    /// Conservative: -1% (tight stop), Aggressive: -12% (wide room)
    pub fn calculate_max_loss_per_trade_pct(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(-0.01), dec!(-0.12))
    }

    /// Take-profit target as percentage (e.g. 0.05 = 5%)
    /// Conservative: 3% (lock gains early), Aggressive: 25% (let winners run)
    pub fn calculate_take_profit_pct(&self) -> Decimal {
        Self::interpolate(self.score, 1, 9, dec!(0.03), dec!(0.25))
    }

    /// Consecutive losing trades before circuit breaker halts (conservative: 2, aggressive: 6).
    pub fn calculate_consecutive_loss_limit(&self) -> usize {
        // 2 + (score-1)*4/8 => score 1->2, 5->4, 9->6
        let step = (self.score.saturating_sub(1)) as usize * 4 / 8;
        (2 + step).clamp(2, 6)
    }

    /// Linear interpolation helper
    ///
    /// Maps a score within [score_min, score_max] to a value within [value_min, value_max]
    fn interpolate(
        score: u8,
        score_min: u8,
        score_max: u8,
        value_min: Decimal,
        value_max: Decimal,
    ) -> Decimal {
        let score_range = Decimal::from(score_max - score_min);
        let score_offset = Decimal::from(score - score_min);
        let ratio = score_offset / score_range;
        value_min + ratio * (value_max - value_min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requires_macd_rising() {
        let conservative = RiskAppetite::new(1).unwrap();
        let balanced = RiskAppetite::new(5).unwrap();
        let aggressive = RiskAppetite::new(9).unwrap();

        assert!(conservative.requires_macd_rising());
        assert!(
            !balanced.requires_macd_rising(),
            "Score 5 should NOT require MACD rising"
        );
        assert!(!aggressive.requires_macd_rising());
    }

    #[test]
    fn test_calculate_trend_tolerance_pct() {
        let conservative = RiskAppetite::new(1).unwrap();
        let balanced = RiskAppetite::new(5).unwrap();
        let aggressive = RiskAppetite::new(9).unwrap();

        assert_eq!(conservative.calculate_trend_tolerance_pct(), Decimal::ZERO);
        // 0.0 + 0.5 * (0.15 - 0.0) = 0.075
        assert_eq!(balanced.calculate_trend_tolerance_pct(), dec!(0.075));
        assert_eq!(aggressive.calculate_trend_tolerance_pct(), dec!(0.15));
    }

    #[test]
    fn test_calculate_macd_min_threshold() {
        let conservative = RiskAppetite::new(1).unwrap();
        let balanced = RiskAppetite::new(5).unwrap();
        let aggressive = RiskAppetite::new(9).unwrap();

        assert_eq!(conservative.calculate_macd_min_threshold(), dec!(0.02));
        // 0.02 + 0.5 * (-0.05 - 0.02) = 0.02 + 0.5 * (-0.07) = 0.02 - 0.035 = -0.015
        assert_eq!(balanced.calculate_macd_min_threshold(), dec!(-0.015));
        assert_eq!(aggressive.calculate_macd_min_threshold(), dec!(-0.05));
    }

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

        // Score 2: 1/8 of the way from min to max (0.003->0.28, 1.2->10, 0.02->1.0)
        let risk_per_trade = risk.calculate_risk_per_trade_percent();
        assert!(risk_per_trade >= dec!(0.003) && risk_per_trade <= dec!(0.05));
        let trailing_stop = risk.calculate_trailing_stop_multiplier();
        assert!(trailing_stop >= dec!(1.2) && trailing_stop <= dec!(3.0));
        let rsi_threshold = risk.calculate_rsi_threshold();
        assert_eq!(rsi_threshold, dec!(58.75));
        let max_position = risk.calculate_max_position_size_pct();
        assert!(max_position >= dec!(0.02) && max_position <= dec!(0.15));
        assert_eq!(risk.calculate_signal_confirmation_bars(), 3);
    }

    #[test]
    fn test_balanced_profile_parameters() {
        let risk = RiskAppetite::new(5).unwrap();

        // Score 5 is mid-range (0.5)
        let risk_per_trade = risk.calculate_risk_per_trade_percent();
        assert!(risk_per_trade >= dec!(0.1) && risk_per_trade <= dec!(0.16));
        let trailing_stop = risk.calculate_trailing_stop_multiplier();
        assert!(trailing_stop >= dec!(5.0) && trailing_stop <= dec!(6.0));
        let rsi_threshold = risk.calculate_rsi_threshold();
        assert_eq!(rsi_threshold, dec!(70.0));
        let max_position = risk.calculate_max_position_size_pct();
        assert!(max_position >= dec!(0.5) && max_position <= dec!(0.55));
        assert!(!risk.requires_macd_rising());
        assert_eq!(risk.calculate_signal_confirmation_bars(), 1);
    }

    #[test]
    fn test_aggressive_profile_parameters() {
        let risk = RiskAppetite::new(9).unwrap();

        let risk_per_trade = risk.calculate_risk_per_trade_percent();
        assert_eq!(risk_per_trade, dec!(0.28));
        let trailing_stop = risk.calculate_trailing_stop_multiplier();
        assert_eq!(trailing_stop, dec!(10.0));
        let rsi_threshold = risk.calculate_rsi_threshold();
        assert_eq!(rsi_threshold, dec!(85.0));
        let max_position = risk.calculate_max_position_size_pct();
        assert_eq!(max_position, dec!(1.00));
        assert_eq!(risk.calculate_signal_confirmation_bars(), 1);
    }

    #[test]
    fn test_max_loss_per_trade_and_take_profit() {
        let conservative = RiskAppetite::new(1).unwrap();
        let aggressive = RiskAppetite::new(9).unwrap();
        assert!(
            conservative.calculate_max_loss_per_trade_pct()
                > aggressive.calculate_max_loss_per_trade_pct()
        );
        assert!(conservative.calculate_max_loss_per_trade_pct() >= dec!(-0.01));
        assert!(aggressive.calculate_max_loss_per_trade_pct() <= dec!(-0.12));
        assert!(conservative.calculate_take_profit_pct() < aggressive.calculate_take_profit_pct());
        assert_eq!(conservative.calculate_consecutive_loss_limit(), 2);
        assert_eq!(aggressive.calculate_consecutive_loss_limit(), 6);
        assert_eq!(conservative.calculate_signal_confirmation_bars(), 3);
        assert_eq!(aggressive.calculate_signal_confirmation_bars(), 1);
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
            risk7.calculate_trailing_stop_multiplier() < risk9.calculate_trailing_stop_multiplier()
        );
    }
}
