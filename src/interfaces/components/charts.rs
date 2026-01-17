use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// Renders a simple donut chart
pub fn render_donut_chart(ui: &mut egui::Ui, percentage: f32, color: egui::Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());

    // Background track
    ui.painter().circle_stroke(
        rect.center(),
        size / 2.0 - 2.0,
        egui::Stroke::new(4.0, DesignSystem::BORDER_SUBTLE),
    );

    if percentage > 0.0 {
        use egui::epaint::{PathShape, Stroke};
        use std::f32::consts::PI;

        let center = rect.center();
        let radius = size / 2.0 - 2.0;
        let start_angle = -PI / 2.0; // Top
        let sweep_angle = 2.0 * PI * (percentage / 100.0);

        let steps = 32;
        let mut points = Vec::new();

        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let angle = start_angle + t * sweep_angle;
            points.push(egui::pos2(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            ));
        }

        ui.painter()
            .add(PathShape::line(points, Stroke::new(4.0, color)));
    }
}
