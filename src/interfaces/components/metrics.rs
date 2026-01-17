use crate::interfaces::components::card::Card;
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// A specialized card for displaying a key metric
pub fn render_metric_card(
    ui: &mut egui::Ui,
    title: &str,
    value: &str,
    value_color: egui::Color32,
    context: Option<&str>,
    icon: Option<&str>,
    active: bool,
) {
    Card::new()
        .title(title)
        .min_height(100.0)
        .active(active)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(value)
                            .size(28.0)
                            .strong()
                            .color(value_color),
                    );

                    if let Some(ctx) = context {
                        ui.label(
                            egui::RichText::new(ctx)
                                .size(11.0)
                                .color(DesignSystem::TEXT_MUTED),
                        );
                    }
                });

                if let Some(emoji) = icon {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(emoji)
                                .size(24.0)
                                .color(DesignSystem::TEXT_MUTED),
                        );
                    });
                }
            });
        });
}

/// A status pill (e.g., for P&L percent)
pub fn render_status_pill(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::NONE
        .fill(color.linear_multiply(0.15))
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(12.0).strong().color(color));
        });
}
