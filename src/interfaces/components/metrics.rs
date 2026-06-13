use crate::interfaces::components::card::Card;
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// A specialized card for displaying a key metric
#[allow(clippy::too_many_arguments)]
pub fn render_metric_card(
    ui: &mut egui::Ui,
    title: &str,
    value: &str,
    value_color: egui::Color32,
    context: Option<&str>,
    icon: Option<&str>,
    active: bool,
    min_height: f32,
) {
    Card::new()
        .title(title)
        .min_height(min_height)
        .active(active)
        .show(ui, |ui| {
            let available_h = ui.available_height();
            let viewport_w = ui.ctx().viewport_rect().width();
            let sidebar_w = (viewport_w * 0.08).clamp(80.0, 120.0);
            let est_col_w = (viewport_w - sidebar_w - 32.0) / 5.0;
            let show_details = available_h > 45.0 && est_col_w > 100.0;

            let font_size = if est_col_w < 90.0 {
                12.0
            } else if est_col_w < 120.0 {
                16.0
            } else if est_col_w < 150.0 {
                22.0
            } else {
                28.0
            };

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(value)
                                .size(font_size)
                                .strong()
                                .color(value_color),
                        )
                        .wrap(),
                    );

                    if show_details && let Some(ctx) = context {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(ctx)
                                    .size(11.0)
                                    .color(DesignSystem::TEXT_MUTED),
                            )
                            .wrap(),
                        );
                    }
                });

                if show_details && let Some(emoji) = icon {
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
