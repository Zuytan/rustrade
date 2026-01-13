use crate::domain::listener::NewsEvent;
use eframe::egui;
use std::collections::VecDeque;

/// Helper function to render the news feed widget
pub fn render_news_feed(ui: &mut egui::Ui, events: &VecDeque<NewsEvent>) {
    egui::ScrollArea::vertical()
        .id_salt("news_feed_scroll")
        .max_height(150.0)
        .show(ui, |ui| {
            if events.is_empty() {
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(28, 33, 40))
                    .corner_radius(6)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Waiting for news...")
                                .color(egui::Color32::from_gray(120))
                                .italics(),
                        );
                    });
            } else {
                for (i, event) in events.iter().enumerate() {
                    // Alternate row background
                    let bg_color = if i % 2 == 0 {
                        egui::Color32::from_rgb(28, 33, 40)
                    } else {
                        egui::Color32::from_rgb(22, 27, 34)
                    };

                    // Sentiment color based on score
                    let sentiment_color = match event.sentiment_score {
                        Some(score) if score > 0.3 => egui::Color32::from_rgb(0, 230, 118), // Green
                        Some(score) if score < -0.3 => egui::Color32::from_rgb(255, 23, 68), // Red
                        _ => egui::Color32::from_gray(140), // Neutral gray
                    };

                    let sentiment_label = match event.sentiment_score {
                        Some(score) if score > 0.3 => "📈 Bullish",
                        Some(score) if score < -0.3 => "📉 Bearish",
                        _ => "➖ Neutral",
                    };

                    egui::Frame::NONE
                        .fill(bg_color)
                        .corner_radius(6)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            // Header: Source & Timestamp
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&event.source)
                                        .size(10.0)
                                        .strong()
                                        .color(egui::Color32::from_rgb(88, 166, 255)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(
                                                event.timestamp.format("%H:%M").to_string(),
                                            )
                                            .size(9.0)
                                            .color(egui::Color32::from_gray(100)),
                                        );
                                    },
                                );
                            });

                            ui.add_space(4.0);

                            // Title
                            ui.label(
                                egui::RichText::new(&event.title)
                                    .size(11.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            );

                            ui.add_space(4.0);

                            // Sentiment badge
                            egui::Frame::NONE
                                .fill(sentiment_color.linear_multiply(0.15))
                                .corner_radius(10)
                                .inner_margin(egui::Margin::symmetric(8, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(sentiment_label)
                                            .size(9.0)
                                            .color(sentiment_color),
                                    );
                                });
                        });

                    ui.add_space(6.0);
                }
            }
        });
}
