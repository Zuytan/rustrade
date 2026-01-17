use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RiskSettings {
    pub max_position_size_pct: String,
    pub max_daily_loss_pct: String,
    pub max_drawdown_pct: String,
    pub consecutive_loss_limit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalystSettings {
    pub strategy_mode: String, // Strategy selected based on risk
    pub fast_sma_period: String,
    pub slow_sma_period: String,
    pub rsi_period: String,
    pub rsi_threshold: String,
    pub macd_min_threshold: String,
    pub adx_threshold: String,
    pub min_profit_ratio: String,
    pub sma_threshold: String,
    pub profit_target_multiplier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSettings {
    pub config_mode: String, // "Simple" or "Advanced"
    pub risk_score: u8,
    pub risk: RiskSettings,
    pub analyst: AnalystSettings,
}

pub struct SettingsPersistence {
    file_path: PathBuf,
}

impl SettingsPersistence {
    pub fn new() -> Result<Self> {
        // Use ~/.rustrade or a similar hidden directory in user home
        let home = std::env::var("HOME").context("Could not find HOME directory")?;
        let config_dir = PathBuf::from(home).join(".rustrade");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).context("Failed to create config directory")?;
        }

        Ok(Self {
            file_path: config_dir.join("settings.json"),
        })
    }

    pub fn load(&self) -> Result<Option<PersistedSettings>> {
        if !self.file_path.exists() {
            return Ok(None);
        }

        let content =
            fs::read_to_string(&self.file_path).context("Failed to read settings file")?;
        let settings: PersistedSettings =
            serde_json::from_str(&content).context("Failed to parse settings JSON")?;

        info!("Loaded settings from {:?}", self.file_path);
        Ok(Some(settings))
    }

    pub fn save(&self, settings: &PersistedSettings) -> Result<()> {
        let content =
            serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;

        // Atomic write: write to temp file then rename
        let temp_path = self.file_path.with_extension("tmp");
        fs::write(&temp_path, content).context("Failed to write temp settings file")?;
        fs::rename(&temp_path, &self.file_path).context("Failed to rename settings file")?;

        info!("Saved settings to {:?}", self.file_path);
        Ok(())
    }
}
