use eframe::egui;

/// Helper function to render a metric card (Concept Art Style)
pub fn render_metric_card(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    value: &str,
    subtitle: Option<&str>,
    value_color: egui::Color32,
    accent_color: egui::Color32,
) {
    // Standard Card Size
    let card_size = egui::vec2(190.0, 100.0);

    ui.allocate_ui_with_layout(card_size, egui::Layout::top_down(egui::Align::LEFT), |ui| {
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(22, 27, 34)) // Dark Card BG
            .inner_margin(egui::Margin::same(12))
            .corner_radius(8)
            .shadow(egui::epaint::Shadow {
                offset: [0, 4],
                blur: 16,
                spread: 0,
                color: egui::Color32::from_black_alpha(100),
            })
            // Top Accent Line
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(10)))
            .show(ui, |ui| {
                ui.set_width(166.0);
                ui.set_height(76.0);

                // Row 1: Title (Left) + Icon (Right)
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(title.to_uppercase())
                            .size(10.0)
                            .color(egui::Color32::from_gray(140))
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Small Faded Icon
                        ui.label(
                            egui::RichText::new(icon)
                                .size(14.0)
                                .color(accent_color.linear_multiply(0.8)),
                        );
                    });
                });

                ui.add_space(6.0);

                // Row 2: Value (Big, Center/Left)
                ui.label(
                    egui::RichText::new(value)
                        .size(24.0)
                        .strong()
                        .color(value_color),
                );

                // Row 3: Sparkline / Subtitle
                if let Some(sub) = subtitle {
                    ui.add_space(8.0);
                    // If it's a P&L card (indicated by color green/red), show sparkline
                    let is_pnl = value_color == egui::Color32::from_rgb(87, 171, 90)
                        || value_color == egui::Color32::from_rgb(248, 81, 73);

                    if is_pnl {
                        let is_positive = value_color == egui::Color32::from_rgb(87, 171, 90);
                        let points = if is_positive {
                            vec![
                                egui::pos2(0.0, 15.0),
                                egui::pos2(10.0, 12.0),
                                egui::pos2(20.0, 14.0),
                                egui::pos2(30.0, 8.0),
                                egui::pos2(40.0, 10.0),
                                egui::pos2(50.0, 2.0),
                            ]
                        } else {
                            vec![
                                egui::pos2(0.0, 2.0),
                                egui::pos2(10.0, 5.0),
                                egui::pos2(20.0, 4.0),
                                egui::pos2(30.0, 10.0),
                                egui::pos2(40.0, 12.0),
                                egui::pos2(50.0, 15.0),
                            ]
                        };

                        ui.horizontal(|ui| {
                            let (response, painter) =
                                ui.allocate_painter(egui::vec2(60.0, 20.0), egui::Sense::hover());
                            let to_screen = egui::emath::RectTransform::from_to(
                                egui::Rect::from_min_size(egui::Pos2::ZERO, response.rect.size()),
                                response.rect,
                            );
                            let screen_points: Vec<egui::Pos2> =
                                points.iter().map(|p| to_screen.transform_pos(*p)).collect();
                            painter.add(egui::Shape::line(
                                screen_points,
                                egui::Stroke::new(2.0, value_color),
                            ));

                            ui.label(
                                egui::RichText::new(sub)
                                    .size(10.0)
                                    .color(egui::Color32::from_gray(120)),
                            );
                        });
                    } else {
                        // Normal subtitle (Win Rate, Total Volume etc)
                        ui.label(
                            egui::RichText::new(sub)
                                .size(10.0)
                                .color(egui::Color32::from_gray(120)),
                        );
                    }
                }
            });
    });
}

pub fn render_mini_metric(ui: &mut egui::Ui, label: String, value: &str, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .size(9.0)
                .color(egui::Color32::from_gray(120)),
        );
        ui.label(egui::RichText::new(value).size(16.0).strong().color(color));
    });
}
