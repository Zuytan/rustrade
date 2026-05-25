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
    SnnSurrogate,
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
            "snnsurrogate" | "snn_surrogate" | "snn-surrogate" | "surrogate" => {
                Ok(StrategyMode::SnnSurrogate)
            }

            _ => anyhow::bail!(
                "Invalid STRATEGY_MODE: {}. Valid: regimeadaptive, smc, ensemble, zscoremr, statmomentum, orderflow, ml, snn_surrogate",
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
            StrategyMode::SnnSurrogate => write!(f, "SnnSurrogate"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_strategy_mode_snn_variants() {
        assert_eq!(
            StrategyMode::from_str("snnsurrogate").unwrap(),
            StrategyMode::SnnSurrogate
        );
        assert_eq!(
            StrategyMode::from_str("snn_surrogate").unwrap(),
            StrategyMode::SnnSurrogate
        );
        assert_eq!(
            StrategyMode::from_str("snn-surrogate").unwrap(),
            StrategyMode::SnnSurrogate
        );
        assert_eq!(
            StrategyMode::from_str("surrogate").unwrap(),
            StrategyMode::SnnSurrogate
        );
    }

    #[test]
    fn test_strategy_mode_display_snn() {
        assert_eq!(StrategyMode::SnnSurrogate.to_string(), "SnnSurrogate");
    }
}
