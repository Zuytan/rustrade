use super::sidebar::{ConfigMode, SettingsTab};
use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::risk::risk_appetite::RiskAppetite;
use crate::domain::risk::risk_config::RiskConfig;
use crate::infrastructure::settings_persistence::{PersistedSettings, SettingsPersistence};
use rust_decimal::Decimal;
use tracing::{error, info};

/// Settings Panel state
pub struct SettingsPanel {
    pub active_tab: SettingsTab,
    pub config_mode: ConfigMode, // NEW
    pub risk_score: u8,          // NEW: 1-10
    pub selected_strategy: crate::domain::market::strategy_config::StrategyMode, // Auto-selected based on risk

    // --- Risk Management ---
    pub max_position_size_pct: String,
    pub max_daily_loss_pct: String,
    pub max_drawdown_pct: String,       // NEW
    pub consecutive_loss_limit: String, // NEW

    // --- Strategy: Trend (SMA) ---
    pub fast_sma_period: String, // NEW
    pub slow_sma_period: String, // NEW

    // --- Strategy: Oscillators ---
    pub rsi_period: String, // NEW
    pub rsi_threshold: String,

    // --- Strategy: MACD ---
    pub macd_min_threshold: String, // NEW

    // --- Strategy: Advanced ---
    pub adx_threshold: String,    // NEW
    pub min_profit_ratio: String, // NEW

    pub sma_threshold: String,
    pub profit_target_multiplier: String,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsPanel {
    pub fn new() -> Self {
        let mut panel = Self {
            active_tab: SettingsTab::TradingEngine,
            config_mode: ConfigMode::Simple, // Default to simple for novices
            risk_score: 5,                   // Default balanced score
            selected_strategy: crate::domain::market::strategy_config::StrategyMode::RegimeAdaptive, // Default for risk 5

            // Risk Defaults
            max_position_size_pct: "0.10".to_string(),
            max_daily_loss_pct: "0.02".to_string(),
            max_drawdown_pct: "0.05".to_string(),
            consecutive_loss_limit: "3".to_string(),

            // Strategy Defaults
            fast_sma_period: "10".to_string(),
            slow_sma_period: "20".to_string(),
            rsi_period: "14".to_string(),
            rsi_threshold: "70.0".to_string(),

            macd_min_threshold: "0.0".to_string(),
            adx_threshold: "25.0".to_string(),
            min_profit_ratio: "1.5".to_string(),

            sma_threshold: "0.001".to_string(),
            profit_target_multiplier: "2.0".to_string(),
        };
        // Initialize strings based on default risk score
        panel.update_from_score(5);

        // Try to load persisted settings
        match SettingsPersistence::new() {
            Ok(persistence) => match persistence.load() {
                Ok(Some(settings)) => {
                    info!("Applying persisted settings");
                    panel.apply_persisted_settings(&settings);
                }
                Ok(None) => info!("No persisted settings found, using defaults"),
                Err(e) => error!("Failed to load settings: {}", e),
            },
            Err(e) => error!("Failed to initialize settings persistence: {}", e),
        }

        panel
    }

    /// Applies persisted settings to the panel
    pub fn apply_persisted_settings(&mut self, settings: &PersistedSettings) {
        // Mode & Score
        self.config_mode = match settings.config_mode.as_str() {
            "Advanced" => ConfigMode::Advanced,
            _ => ConfigMode::Simple,
        };
        self.risk_score = settings.risk_score;

        // Strategy Mode
        use crate::domain::market::strategy_config::StrategyMode;
        self.selected_strategy = match settings.analyst.strategy_mode.as_str() {
            "SMC" => StrategyMode::SMC,
            "RegimeAdaptive" => StrategyMode::RegimeAdaptive,
            "Standard" => StrategyMode::Standard,
            "Momentum" => StrategyMode::Momentum,
            "MeanReversion" => StrategyMode::MeanReversion,
            "Breakout" => StrategyMode::Breakout,
            "TrendRiding" => StrategyMode::TrendRiding,
            "Advanced" => StrategyMode::Advanced,
            "Dynamic" => StrategyMode::Dynamic,
            "VWAP" => StrategyMode::VWAP,
            "Ensemble" => StrategyMode::Ensemble,
            _ => Self::select_strategy_for_risk(settings.risk_score), // Fallback to risk-based
        };

        // Risk Settings
        self.max_position_size_pct = settings.risk.max_position_size_pct.clone();
        self.max_daily_loss_pct = settings.risk.max_daily_loss_pct.clone();
        self.max_drawdown_pct = settings.risk.max_drawdown_pct.clone();
        self.consecutive_loss_limit = settings.risk.consecutive_loss_limit.clone();

        // Analyst Settings
        self.fast_sma_period = settings.analyst.fast_sma_period.clone();
        self.slow_sma_period = settings.analyst.slow_sma_period.clone();
        self.rsi_period = settings.analyst.rsi_period.clone();
        self.rsi_threshold = settings.analyst.rsi_threshold.clone();
        self.macd_min_threshold = settings.analyst.macd_min_threshold.clone();
        self.adx_threshold = settings.analyst.adx_threshold.clone();
        self.min_profit_ratio = settings.analyst.min_profit_ratio.clone();
        self.sma_threshold = settings.analyst.sma_threshold.clone();
        self.profit_target_multiplier = settings.analyst.profit_target_multiplier.clone();
    }

    /// Maps risk score to optimal strategy based on benchmark results
    fn select_strategy_for_risk(score: u8) -> crate::domain::market::strategy_config::StrategyMode {
        use crate::domain::market::strategy_config::StrategyMode;
        match score {
            1..=3 => StrategyMode::Standard, // Conservative: Safe, avoids chop
            4..=6 => StrategyMode::RegimeAdaptive, // Balanced: Steady gains
            7..=10 => StrategyMode::SMC,     // Aggressive: Best alpha generator
            _ => StrategyMode::Standard,     // Fallback
        }
    }

    /// Updates all text fields based on the selected risk score (Logic mirroring RiskAppetite domain)
    /// Note: This does NOT change the selected strategy - that's a user choice.
    pub fn update_from_score(&mut self, score: u8) {
        // Strategy selection is a USER choice - do NOT override it here
        // The strategy is only auto-selected on initial panel creation if not loaded from settings
        if let Ok(risk) = RiskAppetite::new(score) {
            // -- Risk --
            self.max_position_size_pct = format!("{:.2}", risk.calculate_max_position_size_pct());

            // Derived Risk Params (not strictly in RiskAppetite struct but inferred logic)
            // Conservative (1) -> Lower Daily Loss (1%), Aggressive (10) -> Higher (5%)
            let max_daily_loss = 0.01 + (score as f64 - 1.0) * (0.04 / 9.0);
            self.max_daily_loss_pct = format!("{:.2}", max_daily_loss);

            // Max Drawdown: Cons 3% -> Aggr 15%
            let max_dd = 0.03 + (score as f64 - 1.0) * (0.12 / 9.0);
            self.max_drawdown_pct = format!("{:.2}", max_dd);

            // Consecutive Loss: Cons 2 -> Aggr 6
            let cons_loss = 2 + ((score as f64 - 1.0) * (4.0 / 9.0)).round() as usize;
            self.consecutive_loss_limit = cons_loss.to_string();

            // -- Strategy --
            self.rsi_threshold = format!("{:.1}", risk.calculate_rsi_threshold());
            self.macd_min_threshold = format!("{:.3}", risk.calculate_macd_min_threshold());
            self.min_profit_ratio = format!("{:.2}", risk.calculate_min_profit_ratio());
            self.profit_target_multiplier =
                format!("{:.2}", risk.calculate_profit_target_multiplier());

            // Inferred Strategy Params
            // ADX: Cons 30 (High quality) -> Aggr 15 (Chop)
            let adx = 30.0 - (score as f64 - 1.0) * (15.0 / 9.0);
            self.adx_threshold = format!("{:.1}", adx);

            // SMA: Cons Slower (20/50) -> Aggr Faster (5/15)
            // Linear interp for Fast: 20 -> 5
            let fast = 20.0 - (score as f64 - 1.0) * (15.0 / 9.0);
            // Linear interp for Slow: 50 -> 15
            let slow = 50.0 - (score as f64 - 1.0) * (35.0 / 9.0);

            self.fast_sma_period = format!("{}", fast.round() as usize);
            self.slow_sma_period = format!("{}", slow.round() as usize);
        }
    }

    /// Converts current UI state to RiskConfig
    pub fn to_risk_config(&self) -> RiskConfig {
        use rust_decimal_macros::dec;
        RiskConfig {
            max_position_size_pct: self.max_position_size_pct.parse().unwrap_or(dec!(0.10)),
            max_daily_loss_pct: self.max_daily_loss_pct.parse().unwrap_or(dec!(0.02)),
            max_drawdown_pct: self.max_drawdown_pct.parse().unwrap_or(dec!(0.05)),
            consecutive_loss_limit: self.consecutive_loss_limit.parse().unwrap_or(3),
            ..RiskConfig::default()
        }
    }

    /// Converts current UI state to AnalystConfig
    pub fn to_analyst_config(&self) -> AnalystConfig {
        use rust_decimal_macros::dec;
        AnalystConfig {
            strategy_mode: self.selected_strategy, // Include selected strategy
            fast_sma_period: self.fast_sma_period.parse().unwrap_or(10),
            slow_sma_period: self.slow_sma_period.parse().unwrap_or(20),
            sma_threshold: self.sma_threshold.parse().unwrap_or(dec!(0.001)),
            rsi_period: self.rsi_period.parse().unwrap_or(14),
            rsi_threshold: self.rsi_threshold.parse().unwrap_or(dec!(70.0)),
            macd_min_threshold: self.macd_min_threshold.parse().unwrap_or(Decimal::ZERO),
            adx_threshold: self.adx_threshold.parse().unwrap_or(dec!(25.0)),
            min_profit_ratio: self.min_profit_ratio.parse().unwrap_or(dec!(1.5)),
            profit_target_multiplier: self.profit_target_multiplier.parse().unwrap_or(dec!(2.0)),
            ..AnalystConfig::default()
        }
    }
}
