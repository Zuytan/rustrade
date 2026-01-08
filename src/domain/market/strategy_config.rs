use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StrategyMode {
    #[default]
    Standard,
    Advanced,
    Dynamic,
    TrendRiding,
    MeanReversion,
    RegimeAdaptive,
    SMC,
}

impl std::str::FromStr for StrategyMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "standard" => Ok(StrategyMode::Standard),
            "advanced" => Ok(StrategyMode::Advanced),
            "dynamic" => Ok(StrategyMode::Dynamic),
            "trendriding" => Ok(StrategyMode::TrendRiding),
            "meanreversion" => Ok(StrategyMode::MeanReversion),
            "regimeadaptive" => Ok(StrategyMode::RegimeAdaptive),
            "smc" => Ok(StrategyMode::SMC),

            _ => anyhow::bail!(
                "Invalid STRATEGY_MODE: {}. Must be 'standard', 'advanced', 'dynamic', 'trendriding', or 'meanreversion'",
                s
            ),
        }
    }
}

impl std::fmt::Display for StrategyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StrategyMode::Standard => write!(f, "Standard"),
            StrategyMode::Advanced => write!(f, "Advanced"),
            StrategyMode::Dynamic => write!(f, "Dynamic"),
            StrategyMode::TrendRiding => write!(f, "TrendRiding"),
            StrategyMode::MeanReversion => write!(f, "MeanReversion"),
            StrategyMode::RegimeAdaptive => write!(f, "RegimeAdaptive"),
            StrategyMode::SMC => write!(f, "SMC"),
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
