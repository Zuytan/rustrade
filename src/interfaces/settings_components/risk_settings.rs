//! Risk settings component (Simple Mode)

use crate::domain::risk::risk_appetite::{RiskAppetite, RiskProfile};
use crate::infrastructure::i18n::I18nService;
use crate::interfaces::components::card::Card;
use crate::interfaces::design_system::DesignSystem;
use crate::interfaces::ui_components::SettingsPanel;
use eframe::egui;

/// Renders the Simple Mode risk settings with score slider
pub fn render_risk_settings(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.add_space(30.0); // Space at top

    Card::new()
        .title(i18n.t("settings_risk_score_label"))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(i18n.t("settings_risk_score_hint"))
                    .color(DesignSystem::TEXT_SECONDARY)
                    .size(14.0),
            );
            ui.add_space(40.0); // More space before slider

            let mut score_f32 = panel.risk_score as f32;

            // Custom styling for slider (wider handle, accent color)
            ui.spacing_mut().slider_width = 300.0;
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
                    RiskProfile::Conservative => ("Conservative (Prudent)", DesignSystem::SUCCESS), // Green
                    RiskProfile::Balanced => ("Balanced (Équilibré)", DesignSystem::WARNING), // Yellow
                    RiskProfile::Aggressive => ("Aggressive (Agressif)", DesignSystem::DANGER), // Red
                };

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Profile:")
                            .size(18.0)
                            .color(DesignSystem::TEXT_PRIMARY),
                    ); // Larger
                    ui.label(
                        egui::RichText::new(profile_text)
                            .strong()
                            .size(18.0)
                            .color(color),
                    ); // Larger
                });

                ui.add_space(30.0); // More space

                // Make derived stats prominent
                egui::Frame::NONE
                    .fill(DesignSystem::BG_INPUT)
                    .corner_radius(DesignSystem::ROUNDING_MEDIUM)
                    .inner_margin(DesignSystem::SPACING_MEDIUM)
                    .show(ui, |ui| {
                        let stats = [
                            (
                                "Risk per Trade",
                                format!(
                                    "{:.1}%",
                                    appetite.calculate_risk_per_trade_percent() * 100.0
                                ),
                            ),
                            (
                                "Max Drawdown",
                                format!(
                                    "{:.1}%",
                                    panel.max_drawdown_pct.parse::<f64>().unwrap_or(0.0) * 100.0
                                ),
                            ),
                            (
                                "Target Profit",
                                format!(
                                    "{:.1}x ATR",
                                    appetite.calculate_profit_target_multiplier()
                                ),
                            ),
                        ];

                        for (label, value) in stats {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(label)
                                        .color(DesignSystem::TEXT_SECONDARY)
                                        .size(14.0),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(value)
                                                .color(DesignSystem::TEXT_PRIMARY)
                                                .strong()
                                                .size(14.0),
                                        );
                                    },
                                );
                            });
                            ui.add_space(4.0);
                        }
                    });
            }
        });

    ui.add_space(30.0); // Space at bottom
}
