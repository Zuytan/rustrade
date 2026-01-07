use crate::domain::ui::I18nService;
use eframe::egui;
use tokio::sync::mpsc::Sender;
use crate::application::risk_management::commands::RiskCommand;
use crate::application::agents::analyst::AnalystCommand;
use crate::application::agents::analyst::AnalystConfig; // Need default or similar
use crate::application::risk_management::risk_manager::RiskConfig;


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

/// Settings Panel state
pub struct SettingsPanel {
    pub active_tab: SettingsTab,
    
    // --- Risk Management ---
    pub max_position_size_pct: String,
    pub max_daily_loss_pct: String,
    pub max_drawdown_pct: String,        // NEW
    pub consecutive_loss_limit: String,  // NEW
    
    // --- Strategy: Trend (SMA) ---
    pub fast_sma_period: String,         // NEW
    pub slow_sma_period: String,         // NEW
    
    // --- Strategy: Oscillators ---
    pub rsi_period: String,              // NEW
    pub rsi_threshold: String,
    
    // --- Strategy: MACD ---
    pub macd_min_threshold: String,      // NEW
    
    // --- Strategy: Advanced ---
    pub adx_threshold: String,           // NEW
    pub min_profit_ratio: String,        // NEW
    
    pub sma_threshold: String,
    pub profit_target_multiplier: String,
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::SystemConfig, 
            
            // Risk Defaults
            max_position_size_pct: "0.10".to_string(), 
            max_daily_loss_pct: "0.02".to_string(),
            max_drawdown_pct: "0.05".to_string(),
            consecutive_loss_limit: "3".to_string(),
            
            // Strategy Defaults (Standard/Analyst Defaults)
            fast_sma_period: "10".to_string(),
            slow_sma_period: "20".to_string(),
            rsi_period: "14".to_string(),
            rsi_threshold: "70.0".to_string(),
            
            macd_min_threshold: "0.0".to_string(),
            adx_threshold: "25.0".to_string(),
            min_profit_ratio: "1.5".to_string(),
            
            sma_threshold: "0.001".to_string(),
            profit_target_multiplier: "2.0".to_string(),
        }
    }
}

/// Helper to render a setting row with a label, input field, and tooltip hint
fn ui_setting_with_hint(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        let _label_response = ui.label(label);
        // Add a (?) hint icon or just attach tooltip to label
        ui.label(egui::RichText::new("(?)").weak().size(10.0)).on_hover_text(hint);
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
             ui.add(egui::TextEdit::singleline(value).desired_width(60.0));
        });
    });
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
                    if ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(view.icon()).size(24.0));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(view.label(i18n)).size(10.0));
                    }).response.interact(egui::Sense::click()).clicked() {
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
    risk_tx: &Sender<RiskCommand>,
    analyst_tx: &Sender<AnalystCommand>,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(150.0);
            if ui.selectable_label(panel.active_tab == SettingsTab::SystemConfig, format!("⚙ {}", i18n.t("settings_system_config_title"))).clicked() {
                panel.active_tab = SettingsTab::SystemConfig;
            }
            ui.separator();
            if ui.selectable_label(panel.active_tab == SettingsTab::Language, i18n.t("tab_language")).clicked() {
                panel.active_tab = SettingsTab::Language;
            }
            if ui.selectable_label(panel.active_tab == SettingsTab::Help, i18n.t("tab_help")).clicked() {
                panel.active_tab = SettingsTab::Help;
            }
            if ui.selectable_label(panel.active_tab == SettingsTab::Shortcuts, i18n.t("tab_shortcuts")).clicked() {
                panel.active_tab = SettingsTab::Shortcuts;
            }
            if ui.selectable_label(panel.active_tab == SettingsTab::About, i18n.t("tab_about")).clicked() {
                panel.active_tab = SettingsTab::About;
            }
        });

        ui.separator();

        ui.vertical(|ui| {
            match panel.active_tab {
                SettingsTab::SystemConfig => {
                    ui.heading(i18n.t("settings_system_config_title"));
                    ui.label(egui::RichText::new(i18n.t("settings_config_description")).weak().size(12.0));
                    ui.add_space(10.0);
                    
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        
                        // --- Risk Management Group ---
                        ui.group(|ui| {
                            ui.label(egui::RichText::new(i18n.t("settings_group_risk")).strong().size(14.0));
                            ui.add_space(5.0);
                            
                            ui_setting_with_hint(ui, i18n.t("settings_risk_max_pos"), &mut panel.max_position_size_pct, 
                                i18n.t("settings_risk_max_pos_hint"));
                            
                            ui_setting_with_hint(ui, i18n.t("settings_risk_max_loss"), &mut panel.max_daily_loss_pct, 
                                i18n.t("settings_risk_max_loss_hint"));
                                
                            ui_setting_with_hint(ui, i18n.t("settings_risk_max_dd"), &mut panel.max_drawdown_pct, 
                                i18n.t("settings_risk_max_dd_hint"));
                                
                            ui_setting_with_hint(ui, i18n.t("settings_risk_consecutive_loss"), &mut panel.consecutive_loss_limit, 
                                i18n.t("settings_risk_consecutive_loss_hint"));
                        });
                        
                        ui.add_space(10.0);
                        
                        // --- Strategy Group ---
                        ui.group(|ui| {
                            ui.label(egui::RichText::new(i18n.t("settings_group_strategy")).strong().size(14.0));
                            ui.add_space(5.0);
                            
                            ui.collapsing(i18n.t("settings_subgroup_trend"), |ui| {
                                ui_setting_with_hint(ui, i18n.t("settings_strat_fast_sma"), &mut panel.fast_sma_period, 
                                    i18n.t("settings_strat_fast_sma_hint"));
                                ui_setting_with_hint(ui, i18n.t("settings_strat_slow_sma"), &mut panel.slow_sma_period, 
                                    i18n.t("settings_strat_slow_sma_hint"));
                                ui_setting_with_hint(ui, i18n.t("settings_strat_sma_thresh"), &mut panel.sma_threshold, 
                                    i18n.t("settings_strat_sma_thresh_hint"));
                            });

                            ui.collapsing(i18n.t("settings_subgroup_oscillators"), |ui| {
                                ui_setting_with_hint(ui, i18n.t("settings_strat_rsi_period"), &mut panel.rsi_period, 
                                    i18n.t("settings_strat_rsi_period_hint"));
                                ui_setting_with_hint(ui, i18n.t("settings_strat_rsi_thresh"), &mut panel.rsi_threshold, 
                                    i18n.t("settings_strat_rsi_thresh_hint"));
                                ui_setting_with_hint(ui, i18n.t("settings_strat_macd_min"), &mut panel.macd_min_threshold, 
                                    i18n.t("settings_strat_macd_min_hint"));
                            });
                            
                            ui.collapsing(i18n.t("settings_subgroup_advanced"), |ui| {
                                ui_setting_with_hint(ui, i18n.t("settings_strat_adx_thresh"), &mut panel.adx_threshold, 
                                    i18n.t("settings_strat_adx_thresh_hint"));
                                ui_setting_with_hint(ui, i18n.t("settings_strat_min_rr"), &mut panel.min_profit_ratio, 
                                    i18n.t("settings_strat_min_rr_hint"));
                                ui_setting_with_hint(ui, i18n.t("settings_strat_profit_mult"), &mut panel.profit_target_multiplier, 
                                    i18n.t("settings_strat_profit_mult_hint"));
                            });
                        });

                        ui.add_space(20.0);
                        
                        if ui.button(egui::RichText::new(i18n.t("settings_save_button")).size(16.0)).clicked() {
                             // --- Parse Values ---
                             let max_pos = panel.max_position_size_pct.parse::<f64>().unwrap_or(0.10);
                             let max_loss = panel.max_daily_loss_pct.parse::<f64>().unwrap_or(0.02);
                             let max_dd = panel.max_drawdown_pct.parse::<f64>().unwrap_or(0.05);
                             let cons_loss = panel.consecutive_loss_limit.parse::<usize>().unwrap_or(3);
                             
                             let fast_sma = panel.fast_sma_period.parse::<usize>().unwrap_or(10);
                             let slow_sma = panel.slow_sma_period.parse::<usize>().unwrap_or(20);
                             let rsi_per = panel.rsi_period.parse::<usize>().unwrap_or(14);
                             let rsi_thresh = panel.rsi_threshold.parse::<f64>().unwrap_or(70.0);
                             let sma_thresh = panel.sma_threshold.parse::<f64>().unwrap_or(0.001);
                             let adx_thresh = panel.adx_threshold.parse::<f64>().unwrap_or(25.0);
                             let min_rr = panel.min_profit_ratio.parse::<f64>().unwrap_or(1.5);
                             let profit_mult = panel.profit_target_multiplier.parse::<f64>().unwrap_or(2.0);
                             let macd_min = panel.macd_min_threshold.parse::<f64>().unwrap_or(0.0);

                             // --- Create & Send Risk Config ---
                             let mut risk_config = RiskConfig::default();
                             risk_config.max_position_size_pct = max_pos;
                             risk_config.max_daily_loss_pct = max_loss;
                             risk_config.max_drawdown_pct = max_dd;
                             risk_config.consecutive_loss_limit = cons_loss;
                             
                             let _ = risk_tx.try_send(RiskCommand::UpdateConfig(Box::new(risk_config)));
                             
                             // --- Create & Send Analyst Config ---
                             let mut analyst_config = AnalystConfig::default();
                             analyst_config.fast_sma_period = fast_sma;
                             analyst_config.slow_sma_period = slow_sma;
                             analyst_config.rsi_period = rsi_per;
                             analyst_config.rsi_threshold = rsi_thresh;
                             analyst_config.sma_threshold = sma_thresh;
                             analyst_config.adx_threshold = adx_thresh;
                             analyst_config.min_profit_ratio = min_rr;
                             analyst_config.profit_target_multiplier = profit_mult;
                             analyst_config.macd_min_threshold = macd_min;
                             
                             // Preserve other essential non-editable defaults that might get zeroed if not set
                             // (AnalystConfig::default lines 147+ covers them)

                             let _ = analyst_tx.try_send(AnalystCommand::UpdateConfig(Box::new(analyst_config)));
                             
                             // Feedback
                             // We could set a flag to show a "Saved" message briefly.
                        }
                    });
                }
                SettingsTab::Language => {
                    ui.heading(i18n.t("tab_language"));
                    ui.label(i18n.t("language_description"));
                    ui.add_space(10.0);
                    
                    let current_code = i18n.current_language_code().to_string();
                    let languages = i18n.available_languages().to_vec();
                    
                    for lang in languages {
                        if ui.selectable_label(current_code == lang.code, format!("{} {}", lang.flag, lang.name)).clicked() {
                            i18n.set_language(&lang.code);
                        }
                    }
                }
                SettingsTab::Help => {
                    ui.heading(i18n.t("tab_help"));
                    ui.label("Rustrade Help Content");
                }
                SettingsTab::Shortcuts => {
                    ui.heading(i18n.t("tab_shortcuts"));
                    ui.label(i18n.t("shortcuts_description"));
                }
                SettingsTab::About => {
                    ui.heading(i18n.t("tab_about"));
                    ui.label(i18n.t("about_description"));
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                }
            }
        });
    });
}
