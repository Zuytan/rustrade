//! Persistence for optimal parameters discovered through optimization.
//!
//! Stores optimal trading parameters for each risk profile to disk,
//! allowing the UI to load and apply them with one click.

use crate::domain::risk::optimal_parameters::{OptimalParameters, OptimalParametersSet};
use crate::domain::risk::risk_appetite::RiskProfile;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::info;

/// Handles persistence of optimal parameters to disk.
pub struct OptimalParametersPersistence {
    file_path: PathBuf,
}

impl OptimalParametersPersistence {
    /// Creates a new persistence handler.
    ///
    /// The optimal parameters are stored in `~/.rustrade/optimal_parameters.json`.
    pub fn new() -> Result<Self> {
        let home = std::env::var("HOME").context("Could not find HOME directory")?;
        let config_dir = PathBuf::from(home).join(".rustrade");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).context("Failed to create config directory")?;
        }

        Ok(Self {
            file_path: config_dir.join("optimal_parameters.json"),
        })
    }

    /// Loads all optimal parameters from disk.
    pub fn load(&self) -> Result<Option<OptimalParametersSet>> {
        if !self.file_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.file_path)
            .context("Failed to read optimal parameters file")?;
        let params: OptimalParametersSet =
            serde_json::from_str(&content).context("Failed to parse optimal parameters JSON")?;

        info!("Loaded optimal parameters from {:?}", self.file_path);
        Ok(Some(params))
    }

    /// Saves all optimal parameters to disk.
    pub fn save(&self, params: &OptimalParametersSet) -> Result<()> {
        let content = serde_json::to_string_pretty(params)
            .context("Failed to serialize optimal parameters")?;

        // Atomic write: write to temp file then rename
        let temp_path = self.file_path.with_extension("tmp");
        fs::write(&temp_path, content).context("Failed to write temp file")?;
        fs::rename(&temp_path, &self.file_path).context("Failed to rename temp file")?;

        info!("Saved optimal parameters to {:?}", self.file_path);
        Ok(())
    }

    /// Gets optimal parameters for a specific risk profile.
    pub fn get_for_profile(&self, profile: RiskProfile) -> Result<Option<OptimalParameters>> {
        match self.load()? {
            Some(set) => Ok(set.get(profile).cloned()),
            None => Ok(None),
        }
    }

    /// Updates or inserts parameters for a single profile.
    pub fn upsert(&self, params: OptimalParameters) -> Result<()> {
        let mut set = self.load()?.unwrap_or_default();
        set.upsert(params);
        self.save(&set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::optimal_parameters::AssetType;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn create_test_persistence() -> (OptimalParametersPersistence, std::path::PathBuf) {
        let unique_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir().join(format!(
            "rustrade_test_{}_{}_{}_persist",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            unique_id
        ));
        fs::create_dir_all(&temp_dir).expect("Failed to create test temp dir");
        let file_path = temp_dir.join("optimal_parameters.json");
        (
            OptimalParametersPersistence {
                file_path: file_path.clone(),
            },
            temp_dir,
        )
    }

    fn cleanup_test_dir(temp_dir: std::path::PathBuf) {
        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let (persistence, temp_dir) = create_test_persistence();
        let result = persistence.load().unwrap();
        assert!(result.is_none());
        cleanup_test_dir(temp_dir);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let (persistence, temp_dir) = create_test_persistence();

        let params = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Balanced,
            20,
            60,
            65.0,
            3.0,
            0.005,
            300,
            "AAPL".to_string(),
            1.5,
            15.0,
            5.0,
            60.0,
            50,
        );

        let mut set = OptimalParametersSet::new();
        set.upsert(params.clone());

        persistence.save(&set).unwrap();

        let loaded = persistence.load().unwrap().unwrap();
        let loaded_params = loaded.get(RiskProfile::Balanced).unwrap();

        assert_eq!(loaded_params.fast_sma_period, 20);
        assert_eq!(loaded_params.slow_sma_period, 60);
        assert_eq!(loaded_params.rsi_threshold, 65.0);
        cleanup_test_dir(temp_dir);
    }

    #[test]
    fn test_get_for_profile() {
        let (persistence, temp_dir) = create_test_persistence();

        let params = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Aggressive,
            30,
            100,
            70.0,
            4.0,
            0.01,
            0,
            "NVDA".to_string(),
            2.0,
            25.0,
            8.0,
            65.0,
            80,
        );

        let mut set = OptimalParametersSet::new();
        set.upsert(params);
        persistence.save(&set).unwrap();

        let result = persistence
            .get_for_profile(RiskProfile::Aggressive)
            .unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().fast_sma_period, 30);

        let none_result = persistence
            .get_for_profile(RiskProfile::Conservative)
            .unwrap();
        assert!(none_result.is_none());
        cleanup_test_dir(temp_dir);
    }

    #[test]
    fn test_upsert() {
        let (persistence, temp_dir) = create_test_persistence();

        let params1 = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Conservative,
            10,
            50,
            60.0,
            2.0,
            0.003,
            600,
            "TSLA".to_string(),
            1.2,
            10.0,
            3.0,
            55.0,
            30,
        );

        persistence.upsert(params1).unwrap();

        let params2 = OptimalParameters::new(
            AssetType::Stock,
            RiskProfile::Conservative,
            15,
            55,
            62.0,
            2.5,
            0.004,
            500,
            "AAPL".to_string(),
            1.8,
            18.0,
            4.0,
            62.0,
            40,
        );

        persistence.upsert(params2).unwrap();

        let loaded = persistence
            .get_for_profile(RiskProfile::Conservative)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.fast_sma_period, 15); // Updated value
        cleanup_test_dir(temp_dir);
    }
}
