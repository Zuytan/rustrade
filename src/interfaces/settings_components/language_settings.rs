//! Language settings component

use crate::infrastructure::i18n::I18nService;
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// Renders the Language settings tab
pub fn render_language_settings(ui: &mut egui::Ui, i18n: &mut I18nService) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(i18n.t("language_description"))
            .color(DesignSystem::TEXT_SECONDARY)
            .size(14.0),
    );
    ui.add_space(20.0);

    let current_code = i18n.current_language_code().to_string();
    let languages = i18n.available_languages().to_vec();

    ui.horizontal(|ui| {
        for lang in languages {
            let is_selected = current_code == lang.code;

            // Custom Card Frame for Language
            let stroke = if is_selected {
                egui::Stroke::new(1.5, DesignSystem::ACCENT_PRIMARY)
            } else {
                egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE)
            };

            let bg_color = if is_selected {
                DesignSystem::ACCENT_PRIMARY.linear_multiply(0.1)
            } else {
                DesignSystem::BG_CARD
            };

            let response = egui::Frame::NONE
                .fill(bg_color)
                .stroke(stroke)
                .corner_radius(DesignSystem::ROUNDING_MEDIUM)
                .inner_margin(egui::Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.set_width(140.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(&lang.flag).size(36.0));
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(&lang.name)
                                .strong()
                                .color(DesignSystem::TEXT_PRIMARY)
                                .size(16.0),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&lang.native_name)
                                .color(DesignSystem::TEXT_SECONDARY)
                                .size(12.0)
                                .italics(),
                        );
                    });
                })
                .response
                .interact(egui::Sense::click());

            if response.clicked() {
                i18n.set_language(&lang.code);
            }
        }
    });
}
