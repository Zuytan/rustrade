use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StrategyMode {
    #[default]
    RegimeAdaptive,
    SMC,
    Ensemble,
    // NEW: Modern statistical strategies
    ZScoreMR,
    StatMomentum,
    OrderFlow,
    ML,
}

impl std::str::FromStr for StrategyMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "regimeadaptive" => Ok(StrategyMode::RegimeAdaptive),
            "smc" => Ok(StrategyMode::SMC),
            "ensemble" => Ok(StrategyMode::Ensemble),
            "zscoremr" => Ok(StrategyMode::ZScoreMR),
            "statmomentum" => Ok(StrategyMode::StatMomentum),
            "orderflow" => Ok(StrategyMode::OrderFlow),
            "ml" => Ok(StrategyMode::ML),

            _ => anyhow::bail!(
                "Invalid STRATEGY_MODE: {}. Valid: regimeadaptive, smc, ensemble, zscoremr, statmomentum, orderflow, ml",
                s
            ),
        }
    }
}

impl std::fmt::Display for StrategyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StrategyMode::RegimeAdaptive => write!(f, "RegimeAdaptive"),
            StrategyMode::SMC => write!(f, "SMC"),
            StrategyMode::Ensemble => write!(f, "Ensemble"),
            StrategyMode::ZScoreMR => write!(f, "ZScoreMR"),
            StrategyMode::StatMomentum => write!(f, "StatMomentum"),
            StrategyMode::OrderFlow => write!(f, "OrderFlow"),
            StrategyMode::ML => write!(f, "ML"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyDefinition {
    pub symbol: String,
    pub mode: StrategyMode,
    pub config_json: String, // Serialized configuration
    pub is_active: bool,
}
