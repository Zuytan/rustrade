use crate::application::agents::user_agent::{
    ActivityEvent, ActivityEventType, EventSeverity, UserAgent,
};
use crate::infrastructure::i18n::I18nService;
use eframe::egui;
use std::collections::VecDeque;

/// Helper function to render the activity feed
pub fn render_activity_feed(
    ui: &mut egui::Ui,
    events: &VecDeque<ActivityEvent>,
    i18n: &I18nService,
) {
    egui::ScrollArea::vertical()
        .id_salt("activity_feed_scroll")
        .max_height(300.0)
        .show(ui, |ui| {
            if events.is_empty() {
                ui.label(
                    egui::RichText::new(i18n.t("no_activity"))
                        .color(egui::Color32::from_gray(120))
                        .italics(),
                );
            } else {
                for (i, event) in events.iter().enumerate() {
                    let icon = match event.event_type {
                        ActivityEventType::TradeExecuted => "✅",
                        ActivityEventType::Signal => "📣",
                        ActivityEventType::FilterBlock => "⛔",
                        ActivityEventType::StrategyChange => "🔧",
                        ActivityEventType::Alert => "⚠️",
                        ActivityEventType::System => "ℹ",
                    };

                    let color = match event.severity {
                        EventSeverity::Info => egui::Color32::from_gray(200),
                        EventSeverity::Warning => egui::Color32::from_rgb(255, 212, 59),
                        EventSeverity::Error => egui::Color32::from_rgb(248, 81, 73),
                    };

                    // Striped Row Background
                    let bg_color = if i % 2 == 0 {
                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 5) // Very subtle light stripe
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    egui::Frame::NONE
                        .fill(bg_color)
                        .inner_margin(4)
                        .corner_radius(2)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(icon).size(12.0));
                                ui.label(
                                    egui::RichText::new(
                                        event.timestamp.format("%H:%M:%S").to_string(),
                                    )
                                    .size(10.0)
                                    .color(egui::Color32::from_gray(120)),
                                );
                                ui.label(
                                    egui::RichText::new(&event.message).size(11.0).color(color),
                                );
                            });
                        });
                }
            }
        });
}

/// Helper function to render the logs panel (Moved from ui.rs)
pub fn render_logs_panel(agent: &mut UserAgent, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("logs_panel")
        .resizable(true)
        .default_height(250.0)
        .min_height(30.0)
        .show_animated(ctx, !agent.logs_collapsed, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(agent.i18n.t("section_system_logs")).size(14.0));
                    ui.add_space(8.0);

                    // Log Level Filter Buttons
                    let filter_button = |ui: &mut egui::Ui, label: &str, is_active: bool| -> bool {
                        let button = egui::Button::new(
                            egui::RichText::new(label).size(10.0).color(if is_active {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::from_gray(160)
                            }),
                        )
                        .fill(if is_active {
                            egui::Color32::from_rgb(56, 139, 253)
                        } else {
                            egui::Color32::from_rgb(33, 38, 45)
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if is_active {
                                egui::Color32::from_rgb(88, 166, 255)
                            } else {
                                egui::Color32::from_rgb(48, 54, 61)
                            },
                        ));
                        ui.add(button).clicked()
                    };

                    if filter_button(
                        ui,
                        agent.i18n.t("filter_all"),
                        agent.log_level_filter.is_none(),
                    ) {
                        agent.log_level_filter = None;
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_info"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_info").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_info").to_string());
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_warn"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_warn").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_warn").to_string());
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_error"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_error").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_error").to_string());
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_debug"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_debug").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_debug").to_string());
                    }
                });

                ui.separator();

                // Log output
                egui::ScrollArea::vertical()
                    .id_salt("logs_scroll")
                    .auto_shrink([false, true])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for (sender, msg) in &agent.chat_history {
                            // Apply log level filter
                            if let Some(ref filter_level) = agent.log_level_filter {
                                // Check if sender is a system message (matches any of the system sender keys)
                                let is_system = sender == agent.i18n.t("sender_system")
                                    || sender == agent.i18n.t("sender_system_error")
                                    || sender == agent.i18n.t("sender_system_warn");
                                if is_system && !msg.contains(filter_level.as_str()) {
                                    continue;
                                }
                            }

                            ui.horizontal_wrapped(|ui| {
                                let (label_key, color) = match sender.as_str() {
                                    s if s == agent.i18n.t("sender_user") => {
                                        ("sender_user", egui::Color32::from_rgb(100, 200, 255))
                                    }
                                    s if s == agent.i18n.t("sender_agent") => {
                                        ("sender_agent", egui::Color32::from_rgb(255, 200, 100))
                                    }
                                    _ => {
                                        if msg.contains("ERROR") {
                                            (
                                                "sender_system_error",
                                                egui::Color32::from_rgb(255, 80, 80),
                                            )
                                        } else if msg.contains("WARN") {
                                            (
                                                "sender_system_warn",
                                                egui::Color32::from_rgb(255, 255, 100),
                                            )
                                        } else {
                                            (
                                                "sender_system",
                                                egui::Color32::from_rgb(150, 150, 150),
                                            )
                                        }
                                    }
                                };
                                ui.label(
                                    egui::RichText::new(agent.i18n.t(label_key))
                                        .color(color)
                                        .strong()
                                        .size(10.0),
                                );
                                ui.label(
                                    egui::RichText::new(msg)
                                        .size(10.0)
                                        .color(egui::Color32::from_gray(200)),
                                );
                            });
                        }
                    });
            });
        });

    // Toggle button for logs (always visible at bottom)
    egui::TopBottomPanel::bottom("logs_toggle")
        .exact_height(25.0)
        .frame(
            egui::Frame::NONE
                .fill(egui::Color32::from_rgb(22, 27, 34))
                .inner_margin(egui::Margin::symmetric(8, 4)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Toggle button to show/hide logs
                let button_text = if agent.logs_collapsed {
                    agent.i18n.t("show_logs")
                } else {
                    agent.i18n.t("hide_logs")
                };
                if ui
                    .button(
                        egui::RichText::new(button_text)
                            .size(11.0)
                            .color(egui::Color32::from_gray(180)),
                    )
                    .clicked()
                {
                    agent.logs_collapsed = !agent.logs_collapsed;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(agent.i18n.tf(
                            "messages_count",
                            &[("count", &agent.chat_history.len().to_string())],
                        ))
                        .size(10.0)
                        .color(egui::Color32::from_gray(140)),
                    );
                });
            });
        });
}
