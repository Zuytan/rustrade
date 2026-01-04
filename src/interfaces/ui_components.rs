use crate::domain::ui::I18nService;
use eframe::egui;

/// Settings tab enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
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
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::Language,
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
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(150.0);
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
