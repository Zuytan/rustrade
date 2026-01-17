# Template: UI Component

Use this template for creating new UI components.

## Location
`src/interfaces/{category}/{component_name}.rs`

## Structure

```rust
use eframe::egui;
use crate::interfaces::view_models::MyViewModel; // If separating view model

/// Renders the {Component Name} component.
///
/// # Arguments
/// * `ui` - The egui Ui region to draw into
/// * `data` - The data required to render this view
pub fn render_{component_name}(ui: &mut egui::Ui, data: &MyData) {
    // 1. Container (Card style)
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(18, 22, 29)) // Card Bg
        .rounding(6.0)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 44, 52)))
        .inner_margin(12.0)
        .show(ui, |ui| {
            
            // 2. Header
            ui.horizontal(|ui| {
                ui.heading("Component Title");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Action").clicked() {
                        // Action logic (or send event)
                    }
                });
            });
            
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // 3. Content
            egui::Grid::new("my_component_grid")
                .num_columns(2)
                .spacing([20.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Label 1:");
                    ui.label(format!("{}", data.value1));
                    ui.end_row();

                    ui.label("Label 2:");
                    ui.colored_label(egui::Color32::GREEN, "+12.5%");
                    ui.end_row();
                });
        });
}
```
