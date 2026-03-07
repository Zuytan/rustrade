use super::settings_state::SettingsPanel;
use crate::infrastructure::i18n::I18nService;
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

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
    Architecture,
    Settings,
}

impl DashboardView {
    pub fn icon(&self) -> &'static str {
        match self {
            DashboardView::Dashboard => "📊",
            DashboardView::Charts => "📈",
            DashboardView::Analytics => "🔬",
            DashboardView::Architecture => "🏗️",
            DashboardView::Settings => "⚙️",
        }
    }

    pub fn label(&self, i18n: &I18nService) -> String {
        match self {
            DashboardView::Dashboard => i18n.t("nav_dashboard").to_string(),
            DashboardView::Charts => i18n.t("nav_charts").to_string(),
            DashboardView::Analytics => i18n.t("nav_analytics").to_string(),
            DashboardView::Architecture => "Architecture".to_string(), // i18n.t("nav_architecture").to_string()
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
            DashboardView::Architecture,
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
