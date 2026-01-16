//! Language settings component

use crate::infrastructure::i18n::I18nService;
use eframe::egui;

/// Renders the Language settings tab
pub fn render_language_settings(ui: &mut egui::Ui, i18n: &mut I18nService) {
    ui.heading(i18n.t("tab_language"));
    ui.label(i18n.t("language_description"));
    ui.add_space(10.0);

    let current_code = i18n.current_language_code().to_string();
    let languages = i18n.available_languages().to_vec();

    for lang in languages {
        if ui
            .selectable_label(
                current_code == lang.code,
                format!("{} {}", lang.flag, lang.name),
            )
            .clicked()
        {
            i18n.set_language(&lang.code);
        }
    }
}
