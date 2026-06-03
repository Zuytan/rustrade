use crate::domain::config::StrategyConfig;
use crate::domain::risk::risk_config::RiskConfig;
use crate::domain::trading::fee_model::{ConstantFeeModel, FeeModel};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn default_fee_model() -> Arc<dyn FeeModel> {
    Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystConfig {
    #[serde(flatten)]
    pub strategy: StrategyConfig,

    #[serde(flatten)]
    pub risk: RiskConfig,

    #[serde(skip, default = "default_fee_model")] // FeeModel is trait object
    pub fee_model: Arc<dyn FeeModel>,
}

impl Default for AnalystConfig {
    fn default() -> Self {
        Self {
            strategy: StrategyConfig::default(),
            risk: RiskConfig::default(),
            fee_model: Arc::new(ConstantFeeModel::new(Decimal::ZERO, Decimal::ZERO)),
        }
    }
}

impl From<crate::config::Config> for AnalystConfig {
    fn from(config: crate::config::Config) -> Self {
        let fee_model = config.create_fee_model();
        Self {
            strategy: config.strategy,
            risk: config.risk,
            fee_model,
        }
    }
}

impl AnalystConfig {
    pub fn apply_risk_appetite(
        &mut self,
        appetite: &crate::domain::risk::risk_appetite::RiskAppetite,
    ) {
        // Delegate to domain models (Note: domain models should ideally handle this themselves,
        // but for now we maintain the logic here and update the embedded configs)

        self.risk.risk_per_trade_percent = appetite.calculate_risk_per_trade_percent();
        self.strategy.trailing_stop_atr_multiplier = appetite.calculate_trailing_stop_multiplier();
        self.strategy.rsi_threshold = appetite.calculate_rsi_threshold();
        self.risk.max_position_size_pct = appetite.calculate_max_position_size_pct();
        self.strategy.min_profit_ratio = appetite.calculate_min_profit_ratio();
        self.strategy.macd_requires_rising = appetite.requires_macd_rising();
        self.strategy.trend_tolerance_pct = appetite.calculate_trend_tolerance_pct();
        self.strategy.macd_min_threshold = appetite.calculate_macd_min_threshold();
        self.strategy.profit_target_multiplier = appetite.calculate_profit_target_multiplier();

        // Risk price: max loss per trade and take-profit (prise de risque)
        self.risk.max_loss_per_trade_pct = appetite.calculate_max_loss_per_trade_pct();
        self.strategy.take_profit_pct = appetite.calculate_take_profit_pct();

        // Stricter effective threshold for conservative => fewer/same signals; aggressive keeps normal.
        let sensitivity = appetite.calculate_signal_sensitivity_factor();
        self.strategy.sma_threshold *= sensitivity;

        // Conservative: more confirmation bars => fewer trades. Aggressive: 1 bar => same as before.
        self.strategy.signal_confirmation_bars = appetite.calculate_signal_confirmation_bars();

        // Adjust Ensemble Consensus Threshold based on Risk Score
        use rust_decimal_macros::dec;
        self.strategy.ensemble_voting_threshold = match appetite.score() {
            1..=2 => dec!(0.60),
            3..=4 => dec!(0.55),
            5..=6 => dec!(0.50),
            7..=8 => dec!(0.40),
            9 => dec!(0.30),
            _ => dec!(0.50), // Fallback
        };

        self.strategy.risk_appetite_score = Some(appetite.score());
    }
}

impl From<&AnalystConfig> for crate::application::risk_management::sizing_engine::SizingConfig {
    fn from(config: &AnalystConfig) -> Self {
        use rust_decimal_macros::dec;
        Self {
            risk_per_trade_percent: config.risk.risk_per_trade_percent,
            max_positions: config.risk.max_positions,
            max_position_size_pct: config.risk.max_position_size_pct,
            static_trade_quantity: config.risk.trade_quantity,
            enable_vol_targeting: false,   // Disabled by default for now
            target_volatility: dec!(0.15), // 15% target if enabled
        }
    }
}
