//! Help, Shortcuts, and About tab components

use crate::infrastructure::i18n::I18nService;
use eframe::egui;

/// Renders the Help settings tab
pub fn render_help_tab(ui: &mut egui::Ui, i18n: &I18nService) {
    ui.heading(i18n.t("tab_help"));
    ui.label("Rustrade Help Content");
}

/// Renders the Shortcuts settings tab
pub fn render_shortcuts_tab(ui: &mut egui::Ui, i18n: &I18nService) {
    ui.heading(i18n.t("tab_shortcuts"));
    ui.label(i18n.t("shortcuts_description"));
}

/// Renders the About settings tab
pub fn render_about_tab(ui: &mut egui::Ui, i18n: &I18nService) {
    ui.heading(i18n.t("tab_about"));
    ui.label(i18n.t("about_description"));
    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
}
