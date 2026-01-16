use crate::application::agents::analyst::AnalystCommand;
use crate::application::agents::analyst::AnalystConfig;
use crate::application::client::SystemClient;
use crate::application::risk_management::commands::RiskCommand;
use crate::domain::risk::risk_appetite::RiskAppetite;
use crate::domain::risk::risk_config::RiskConfig;
use crate::infrastructure::i18n::I18nService;
use crate::interfaces::settings_components;
use eframe::egui;
use tracing::error;

/// Settings tab enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    SystemConfig, // New tab
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
    Portfolio,
    Analytics,
    Settings,
}

impl DashboardView {
    pub fn icon(&self) -> &'static str {
        match self {
            DashboardView::Dashboard => "📊",
            DashboardView::Charts => "📈",
            DashboardView::Portfolio => "💼",
            DashboardView::Analytics => "🔬",
            DashboardView::Settings => "⚙️",
        }
    }

    pub fn label(&self, i18n: &I18nService) -> String {
        match self {
            DashboardView::Dashboard => i18n.t("nav_dashboard").to_string(),
            DashboardView::Charts => i18n.t("nav_charts").to_string(),
            DashboardView::Portfolio => i18n.t("nav_portfolio").to_string(),
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
            active_tab: SettingsTab::SystemConfig,
            config_mode: ConfigMode::Simple, // Default to simple for novices
            risk_score: 5,                   // Default balanced score

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
        panel
    }

    /// Updates all text fields based on the selected risk score (Logic mirroring RiskAppetite domain)
    pub fn update_from_score(&mut self, score: u8) {
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
            DashboardView::Portfolio,
            DashboardView::Analytics,
            DashboardView::Settings,
        ];

        for view in views {
            let is_selected = *current_view == view;

            let bg_color = if is_selected {
                egui::Color32::from_rgb(28, 33, 40)
            } else {
                egui::Color32::TRANSPARENT
            };

            let stroke = if is_selected {
                egui::Stroke::new(1.5, egui::Color32::from_rgb(41, 121, 255))
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
    ui.horizontal(|ui| {
        // Sidebar navigation
        render_settings_sidebar(ui, panel, i18n);

        ui.separator();

        // Content area
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.set_min_width(ui.available_width());

            match panel.active_tab {
                SettingsTab::SystemConfig => {
                    render_system_config_tab(ui, panel, i18n, client);
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
            }
        });
    });
}

/// Renders the settings sidebar navigation
fn render_settings_sidebar(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.vertical(|ui| {
        ui.set_width(150.0);
        if ui
            .selectable_label(
                panel.active_tab == SettingsTab::SystemConfig,
                format!("⚙ {}", i18n.t("settings_system_config_title")),
            )
            .clicked()
        {
            panel.active_tab = SettingsTab::SystemConfig;
        }
        ui.separator();
        if ui
            .selectable_label(
                panel.active_tab == SettingsTab::Language,
                i18n.t("tab_language"),
            )
            .clicked()
        {
            panel.active_tab = SettingsTab::Language;
        }
        if ui
            .selectable_label(panel.active_tab == SettingsTab::Help, i18n.t("tab_help"))
            .clicked()
        {
            panel.active_tab = SettingsTab::Help;
        }
        if ui
            .selectable_label(
                panel.active_tab == SettingsTab::Shortcuts,
                i18n.t("tab_shortcuts"),
            )
            .clicked()
        {
            panel.active_tab = SettingsTab::Shortcuts;
        }
        if ui
            .selectable_label(panel.active_tab == SettingsTab::About, i18n.t("tab_about"))
            .clicked()
        {
            panel.active_tab = SettingsTab::About;
        }
    });
}

/// Renders the System Config tab with mode toggle and scrollable content
fn render_system_config_tab(
    ui: &mut egui::Ui,
    panel: &mut SettingsPanel,
    i18n: &I18nService,
    client: &SystemClient,
) {
    // Header
    ui.vertical(|ui| {
        ui.heading(i18n.t("settings_system_config_title"));
        ui.label(
            egui::RichText::new(i18n.t("settings_config_description"))
                .weak()
                .size(12.0),
        );
    });

    // Mode toggle (Simple/Advanced)
    render_mode_toggle(ui, panel, i18n);

    ui.separator();

    // Scrollable content
    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .min_scrolled_height(600.0)
        .show(ui, |ui| {
            if panel.config_mode == ConfigMode::Simple {
                settings_components::render_risk_settings(ui, panel, i18n);
            } else {
                settings_components::render_strategy_settings(ui, panel, i18n);
            }
            ui.add_space(20.0);
        });

    ui.add_space(15.0);

    // Save button with command sending logic
    render_save_button(ui, panel, i18n, client);
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
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_gray(200)
                })
                .strong()
                .size(14.0),
        )
        .fill(if simple_active {
            egui::Color32::from_rgb(41, 121, 255)
        } else {
            egui::Color32::from_rgb(40, 44, 52)
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
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_gray(200)
                })
                .strong()
                .size(14.0),
        )
        .fill(if advanced_active {
            egui::Color32::from_rgb(41, 121, 255)
        } else {
            egui::Color32::from_rgb(40, 44, 52)
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
        // Parse values
        let max_pos = panel.max_position_size_pct.parse::<f64>().unwrap_or(0.10);
        let max_loss = panel.max_daily_loss_pct.parse::<f64>().unwrap_or(0.02);
        let max_dd = panel.max_drawdown_pct.parse::<f64>().unwrap_or(0.05);
        let cons_loss = panel.consecutive_loss_limit.parse::<usize>().unwrap_or(3);

        let fast_sma = panel.fast_sma_period.parse::<usize>().unwrap_or(10);
        let slow_sma = panel.slow_sma_period.parse::<usize>().unwrap_or(20);
        let sma_thresh = panel.sma_threshold.parse::<f64>().unwrap_or(0.001);

        let rsi_p = panel.rsi_period.parse::<usize>().unwrap_or(14);
        let rsi_t = panel.rsi_threshold.parse::<f64>().unwrap_or(30.0);
        let macd_min = panel.macd_min_threshold.parse::<f64>().unwrap_or(0.0001);

        let adx_t = panel.adx_threshold.parse::<f64>().unwrap_or(25.0);
        let min_rr = panel.min_profit_ratio.parse::<f64>().unwrap_or(1.5);
        let prof_mult = panel.profit_target_multiplier.parse::<f64>().unwrap_or(2.0);

        // Create and send Risk Config
        let risk_config = RiskConfig {
            max_position_size_pct: max_pos,
            max_daily_loss_pct: max_loss,
            max_drawdown_pct: max_dd,
            consecutive_loss_limit: cons_loss,
            ..RiskConfig::default()
        };

        if let Err(e) = client.send_risk_command(RiskCommand::UpdateConfig(Box::new(risk_config))) {
            error!("Failed to send update config command: {}", e);
        }

        // Create and send Analyst Config
        let analyst_cfg = AnalystConfig {
            fast_sma_period: fast_sma,
            slow_sma_period: slow_sma,
            sma_threshold: sma_thresh,
            rsi_period: rsi_p,
            rsi_threshold: rsi_t,
            macd_min_threshold: macd_min,
            adx_threshold: adx_t,
            min_profit_ratio: min_rr,
            profit_target_multiplier: prof_mult,
            ..AnalystConfig::default()
        };

        if let Err(e) =
            client.send_analyst_command(AnalystCommand::UpdateConfig(Box::new(analyst_cfg)))
        {
            error!("Failed to send analyst config update: {}", e);
        }
    }
}
