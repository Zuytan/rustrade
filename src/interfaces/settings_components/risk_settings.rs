//! Risk settings component (Simple Mode)

use crate::domain::risk::risk_appetite::{RiskAppetite, RiskProfile};
use crate::infrastructure::i18n::I18nService;
use crate::interfaces::ui_components::SettingsPanel;
use eframe::egui;

/// Renders the Simple Mode risk settings with score slider
pub fn render_risk_settings(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.add_space(30.0); // Space at top

    ui.group(|ui| {
        ui.heading(egui::RichText::new(i18n.t("settings_risk_score_label")).size(22.0));
        ui.label(
            egui::RichText::new(i18n.t("settings_risk_score_hint"))
                .weak()
                .size(14.0),
        );
        ui.add_space(40.0); // More space before slider

        let mut score_f32 = panel.risk_score as f32;
        // Make slider larger
        let slider = egui::Slider::new(&mut score_f32, 1.0..=10.0)
            .step_by(1.0)
            .show_value(true);
        ui.add(slider);

        if score_f32 as u8 != panel.risk_score {
            panel.risk_score = score_f32 as u8;
            panel.update_from_score(panel.risk_score);
        }

        ui.add_space(40.0); // More space after slider

        // Show derived profile badge
        if let Ok(appetite) = RiskAppetite::new(panel.risk_score) {
            let (profile_text, color) = match appetite.profile() {
                RiskProfile::Conservative => (
                    "Conservative (Prudent)",
                    egui::Color32::from_rgb(100, 200, 100),
                ), // Green
                RiskProfile::Balanced => (
                    "Balanced (Équilibré)",
                    egui::Color32::from_rgb(200, 200, 100),
                ), // Yellow
                RiskProfile::Aggressive => (
                    "Aggressive (Agressif)",
                    egui::Color32::from_rgb(200, 100, 100),
                ), // Red
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Profile:").size(18.0)); // Larger
                ui.colored_label(color, egui::RichText::new(profile_text).strong().size(18.0)); // Larger
            });

            ui.add_space(30.0); // More space

            // Make derived stats prominent
            ui.group(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Risk per Trade: {:.1}%",
                        appetite.calculate_risk_per_trade_percent() * 100.0
                    ))
                    .size(16.0),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Max Drawdown: {:.1}%",
                        panel.max_drawdown_pct.parse::<f64>().unwrap_or(0.0) * 100.0
                    ))
                    .size(16.0),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Target Profit: {:.1}x ATR",
                        appetite.calculate_profit_target_multiplier()
                    ))
                    .size(16.0),
                );
            });
        }
    });

    ui.add_space(30.0); // Space at bottom
}
