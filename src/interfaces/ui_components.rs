use crate::application::agents::analyst::AnalystCommand;
use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::client::SystemClient;
use crate::application::risk_management::commands::RiskCommand;
use crate::domain::risk::risk_appetite::RiskAppetite;
use crate::domain::risk::risk_config::RiskConfig;
use crate::infrastructure::i18n::I18nService;
use crate::infrastructure::settings_persistence::{
    AnalystSettings, PersistedSettings, RiskSettings, SettingsPersistence,
};
use crate::interfaces::design_system::DesignSystem;
use crate::interfaces::settings_components;
use eframe::egui;
use tracing::{error, info};

/// Settings tab enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    TradingEngine, // Renamed from SystemConfig
    Language,
    Help,
    Shortcuts,
    About,
}

/// Dashboard View enumeration for Sidebar Navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardView {
    Dashboard,
    Charts,
    Analytics,
    Settings,
}

impl DashboardView {
    pub fn icon(&self) -> &'static str {
        match self {
            DashboardView::Dashboard => "📊",
            DashboardView::Charts => "📈",
            DashboardView::Analytics => "🔬",
            DashboardView::Settings => "⚙️",
        }
    }

    pub fn label(&self, i18n: &I18nService) -> String {
        match self {
            DashboardView::Dashboard => i18n.t("nav_dashboard").to_string(),
            DashboardView::Charts => i18n.t("nav_charts").to_string(),
            DashboardView::Analytics => i18n.t("nav_analytics").to_string(),
            DashboardView::Settings => i18n.t("nav_settings").to_string(),
        }
    }
}

/// Configuration Mode
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ConfigMode {
    Simple,
    Advanced,
}

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
        RiskConfig {
            max_position_size_pct: self.max_position_size_pct.parse().unwrap_or(0.10),
            max_daily_loss_pct: self.max_daily_loss_pct.parse().unwrap_or(0.02),
            max_drawdown_pct: self.max_drawdown_pct.parse().unwrap_or(0.05),
            consecutive_loss_limit: self.consecutive_loss_limit.parse().unwrap_or(3),
            ..RiskConfig::default()
        }
    }

    /// Converts current UI state to AnalystConfig
    pub fn to_analyst_config(&self) -> AnalystConfig {
        AnalystConfig {
            strategy_mode: self.selected_strategy, // Include selected strategy
            fast_sma_period: self.fast_sma_period.parse().unwrap_or(10),
            slow_sma_period: self.slow_sma_period.parse().unwrap_or(20),
            sma_threshold: self.sma_threshold.parse().unwrap_or(0.001),
            rsi_period: self.rsi_period.parse().unwrap_or(14),
            rsi_threshold: self.rsi_threshold.parse().unwrap_or(70.0),
            macd_min_threshold: self.macd_min_threshold.parse().unwrap_or(0.0),
            adx_threshold: self.adx_threshold.parse().unwrap_or(25.0),
            min_profit_ratio: self.min_profit_ratio.parse().unwrap_or(1.5),
            profit_target_multiplier: self.profit_target_multiplier.parse().unwrap_or(2.0),
            ..AnalystConfig::default()
        }
    }
}

pub fn render_sidebar(
    ui: &mut egui::Ui,
    current_view: &mut DashboardView,
    _settings_panel: &mut SettingsPanel,
    i18n: &I18nService,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);

        let views = [
            DashboardView::Dashboard,
            DashboardView::Charts,
            DashboardView::Analytics,
            DashboardView::Settings,
        ];

        for view in views {
            let is_selected = *current_view == view;

            let bg_color = if is_selected {
                DesignSystem::BG_CARD_HOVER
            } else {
                egui::Color32::TRANSPARENT
            };

            let stroke = if is_selected {
                egui::Stroke::new(1.5, DesignSystem::ACCENT_PRIMARY)
            } else {
                egui::Stroke::NONE
            };

            egui::Frame::NONE
                .fill(bg_color)
                .corner_radius(8)
                .stroke(stroke)
                .inner_margin(egui::Margin::symmetric(0, 12))
                .show(ui, |ui| {
                    ui.set_width(80.0);
                    if ui
                        .vertical_centered(|ui| {
                            ui.label(egui::RichText::new(view.icon()).size(24.0));
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(view.label(i18n)).size(10.0));
                        })
                        .response
                        .interact(egui::Sense::click())
                        .clicked()
                    {
                        *current_view = view;
                    }
                });

            ui.add_space(15.0);
        }
    });
}

pub fn render_settings_view(
    ui: &mut egui::Ui,
    panel: &mut SettingsPanel,
    i18n: &mut I18nService,
    client: &SystemClient,
) {
    let total_height = ui.available_height();

    ui.horizontal(|ui| {
        // --- Sidebar (Left) ---
        egui::Frame::NONE
            .fill(DesignSystem::BG_PANEL)
            .inner_margin(egui::Margin::symmetric(10, 20))
            .show(ui, |ui| {
                ui.set_width(180.0);
                ui.set_min_height(total_height);
                render_settings_sidebar(ui, panel, i18n);
            });

        ui.add_space(1.0);

        // --- Content Area (Right) - allocate remaining space ---
        let content_width = ui.available_width();
        ui.allocate_ui(egui::vec2(content_width, total_height), |ui| {
            ui.vertical(|ui| {
                // Header with title and save button
                ui.horizontal(|ui| {
                    let title = match panel.active_tab {
                        SettingsTab::TradingEngine => i18n.t("settings_system_config_title"),
                        SettingsTab::Language => i18n.t("tab_language"),
                        SettingsTab::Help => i18n.t("tab_help"),
                        SettingsTab::Shortcuts => i18n.t("tab_shortcuts"),
                        SettingsTab::About => i18n.t("tab_about"),
                    };

                    ui.heading(
                        egui::RichText::new(title)
                            .size(24.0)
                            .strong()
                            .color(DesignSystem::TEXT_PRIMARY),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if panel.active_tab == SettingsTab::TradingEngine {
                            render_save_button(ui, panel, i18n, client);
                        }
                    });
                });

                ui.add_space(DesignSystem::SPACING_MEDIUM);
                ui.separator();
                ui.add_space(DesignSystem::SPACING_MEDIUM);

                // ScrollArea that fills remaining space
                let scroll_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .auto_shrink([false, false])
                    .max_height(scroll_height)
                    .show(ui, |ui| match panel.active_tab {
                        SettingsTab::TradingEngine => {
                            render_trading_engine_tab(ui, panel, i18n);
                        }
                        SettingsTab::Language => {
                            settings_components::render_language_settings(ui, i18n);
                        }
                        SettingsTab::Help => {
                            settings_components::render_help_tab(ui, i18n);
                        }
                        SettingsTab::Shortcuts => {
                            settings_components::render_shortcuts_tab(ui, i18n);
                        }
                        SettingsTab::About => {
                            settings_components::render_about_tab(ui, i18n);
                        }
                    });
            });
        });
    });
}

/// Renders the settings sidebar navigation
fn render_settings_sidebar(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        let tabs = [
            (
                SettingsTab::TradingEngine,
                "⚙",
                i18n.t("settings_system_config_title"),
            ),
            (SettingsTab::Language, "🌐", i18n.t("tab_language")),
            (SettingsTab::Shortcuts, "⌨", i18n.t("tab_shortcuts")),
            (SettingsTab::Help, "❓", i18n.t("tab_help")),
            (SettingsTab::About, "ℹ", i18n.t("tab_about")),
        ];

        for (tab, icon, label) in tabs {
            let is_selected = panel.active_tab == tab;

            let bg = if is_selected {
                DesignSystem::ACCENT_PRIMARY.linear_multiply(0.2)
            } else {
                egui::Color32::TRANSPARENT
            };
            let text_color = if is_selected {
                DesignSystem::ACCENT_PRIMARY
            } else {
                DesignSystem::TEXT_SECONDARY
            };
            let border = if is_selected {
                egui::Stroke::new(1.0, DesignSystem::ACCENT_PRIMARY)
            } else {
                egui::Stroke::NONE
            };

            let btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{}  {}", icon, label))
                        .size(14.0)
                        .color(text_color),
                )
                .fill(bg)
                .stroke(border)
                .min_size(egui::vec2(ui.available_width(), 36.0))
                .frame(true),
            );

            if btn.clicked() {
                panel.active_tab = tab;
            }
        }
    });
}

/// Renders the Trading Engine configuration content
fn render_trading_engine_tab(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.label(
        egui::RichText::new(i18n.t("settings_config_description"))
            .color(DesignSystem::TEXT_SECONDARY)
            .size(13.0),
    );
    ui.add_space(DesignSystem::SPACING_MEDIUM);

    // Mode toggle (Simple/Advanced)
    render_mode_toggle(ui, panel, i18n);

    ui.add_space(DesignSystem::SPACING_LARGE);

    if panel.config_mode == ConfigMode::Simple {
        settings_components::render_risk_settings(ui, panel, i18n);
    } else {
        settings_components::render_strategy_settings(ui, panel, i18n);
    }
}

/// Renders the Simple/Advanced mode toggle buttons
fn render_mode_toggle(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} ", i18n.t("settings_mode_label")))
                .strong()
                .size(16.0),
        );

        // Simple Mode Button
        let simple_active = panel.config_mode == ConfigMode::Simple;
        let simple_btn = egui::Button::new(
            egui::RichText::new(i18n.t("settings_mode_simple"))
                .color(if simple_active {
                    DesignSystem::TEXT_PRIMARY
                } else {
                    DesignSystem::TEXT_SECONDARY
                })
                .strong()
                .size(14.0),
        )
        .fill(if simple_active {
            DesignSystem::ACCENT_PRIMARY
        } else {
            DesignSystem::BG_CARD
        })
        .min_size(egui::vec2(120.0, 32.0));

        if ui.add(simple_btn).clicked() {
            panel.config_mode = ConfigMode::Simple;
            panel.update_from_score(panel.risk_score);
        }

        // Advanced Mode Button
        let advanced_active = panel.config_mode == ConfigMode::Advanced;
        let advanced_btn = egui::Button::new(
            egui::RichText::new(i18n.t("settings_mode_advanced"))
                .color(if advanced_active {
                    DesignSystem::TEXT_PRIMARY
                } else {
                    DesignSystem::TEXT_SECONDARY
                })
                .strong()
                .size(14.0),
        )
        .fill(if advanced_active {
            DesignSystem::ACCENT_PRIMARY
        } else {
            DesignSystem::BG_CARD
        })
        .min_size(egui::vec2(120.0, 32.0));

        if ui.add(advanced_btn).clicked() {
            panel.config_mode = ConfigMode::Advanced;
        }
    });
}

/// Renders the save button and handles configuration parsing and sending
fn render_save_button(
    ui: &mut egui::Ui,
    panel: &SettingsPanel,
    i18n: &I18nService,
    client: &SystemClient,
) {
    if ui
        .button(egui::RichText::new(i18n.t("settings_save_button")).size(18.0))
        .clicked()
    {
        // --- Save Settings to Disk ---
        let persisted_settings = PersistedSettings {
            config_mode: match panel.config_mode {
                ConfigMode::Simple => "Simple".to_string(),
                ConfigMode::Advanced => "Advanced".to_string(),
            },
            risk_score: panel.risk_score,
            risk: RiskSettings {
                max_position_size_pct: panel.max_position_size_pct.clone(),
                max_daily_loss_pct: panel.max_daily_loss_pct.clone(),
                max_drawdown_pct: panel.max_drawdown_pct.clone(),
                consecutive_loss_limit: panel.consecutive_loss_limit.clone(),
            },
            analyst: AnalystSettings {
                strategy_mode: format!("{:?}", panel.selected_strategy),
                fast_sma_period: panel.fast_sma_period.clone(),
                slow_sma_period: panel.slow_sma_period.clone(),
                rsi_period: panel.rsi_period.clone(),
                rsi_threshold: panel.rsi_threshold.clone(),
                macd_min_threshold: panel.macd_min_threshold.clone(),
                adx_threshold: panel.adx_threshold.clone(),
                min_profit_ratio: panel.min_profit_ratio.clone(),
                sma_threshold: panel.sma_threshold.clone(),
                profit_target_multiplier: panel.profit_target_multiplier.clone(),
            },
        };

        if let Ok(persistence) = SettingsPersistence::new() {
            if let Err(e) = persistence.save(&persisted_settings) {
                error!("Failed to save settings: {}", e);
            }
        } else {
            error!("Failed to initialize settings persistence for saving");
        }

        // --- Send Updates to System ---
        // Risk Config
        let risk_config = panel.to_risk_config();
        if let Err(e) = client.send_risk_command(RiskCommand::UpdateConfig(Box::new(risk_config))) {
            error!("Failed to send update config command: {}", e);
        }

        // Analyst Config
        let analyst_cfg = panel.to_analyst_config();
        if let Err(e) =
            client.send_analyst_command(AnalystCommand::UpdateConfig(Box::new(analyst_cfg)))
        {
            error!("Failed to send analyst config update: {}", e);
        }
    }
}
