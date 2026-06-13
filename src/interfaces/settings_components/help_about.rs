//! Help, Shortcuts, and About tab components

use crate::infrastructure::i18n::I18nService;
use crate::interfaces::components::card::Card;
use crate::interfaces::design_system::DesignSystem;
use crate::interfaces::ui_components::SettingsPanel;
use eframe::egui;

/// Renders a keyboard keycap visually
fn render_keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(DesignSystem::BG_INPUT)
        .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
        .corner_radius(DesignSystem::ROUNDING_SMALL)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .strong()
                    .monospace()
                    .color(DesignSystem::TEXT_PRIMARY)
                    .size(12.0),
            );
        });
}

/// Renders the Help settings tab with a searchable explorer
pub fn render_help_tab(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.add_space(10.0);

    // Search bar
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("🔍")
                .size(16.0)
                .color(DesignSystem::TEXT_MUTED),
        );

        // Pre-allocated styled input box
        let desired_size = egui::vec2(ui.available_width() - 40.0, 32.0);
        let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

        let id = ui.make_persistent_id("help_search");
        let has_focus = ui.memory(|mem| mem.focused() == Some(id));

        let stroke_color = if has_focus {
            DesignSystem::BORDER_FOCUS
        } else {
            DesignSystem::BORDER_SUBTLE
        };
        ui.painter()
            .rect_filled(rect, DesignSystem::ROUNDING_SMALL, DesignSystem::BG_INPUT);
        ui.painter().rect_stroke(
            rect,
            DesignSystem::ROUNDING_SMALL,
            egui::Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Outside,
        );

        let inner_rect = rect.shrink(4.0);
        ui.put(
            inner_rect,
            egui::TextEdit::singleline(&mut panel.help_search_query)
                .id_source("help_search")
                .hint_text(i18n.t("help_search_placeholder"))
                .font(egui::FontId::proportional(14.0))
                .vertical_align(egui::Align::Center)
                .frame(false),
        );

        if !panel.help_search_query.is_empty() && ui.button("✕").clicked() {
            panel.help_search_query.clear();
        }
    });

    ui.add_space(15.0);

    // Category Selector (Tabs / Chips)
    ui.horizontal_wrapped(|ui| {
        let categories = vec![
            ("all", i18n.t("filter_all")),
            ("abbreviations", i18n.category_name("abbreviations")),
            ("strategies", i18n.category_name("strategies")),
            ("indicators", i18n.category_name("indicators")),
            ("risk_management", i18n.category_name("risk_management")),
            ("order_types", i18n.category_name("order_types")),
        ];

        for (code, label) in categories {
            let is_selected = panel.help_selected_category == code;

            let bg_color = if is_selected {
                DesignSystem::ACCENT_PRIMARY
            } else {
                DesignSystem::BG_CARD
            };

            let text_color = if is_selected {
                DesignSystem::TEXT_PRIMARY
            } else {
                DesignSystem::TEXT_SECONDARY
            };

            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(12.0)
                    .strong()
                    .color(text_color),
            )
            .fill(bg_color)
            .stroke(if is_selected {
                egui::Stroke::NONE
            } else {
                egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE)
            })
            .min_size(egui::vec2(60.0, 26.0));

            if ui.add(btn).clicked() {
                panel.help_selected_category = code.to_string();
            }
            ui.add_space(6.0);
        }
    });

    ui.add_space(20.0);

    // Filter and search topics
    let mut topics = if panel.help_search_query.is_empty() {
        i18n.help_topics()
    } else {
        i18n.search_help(&panel.help_search_query)
    };

    // Filter by category
    if panel.help_selected_category != "all" {
        topics.retain(|topic| topic.category == panel.help_selected_category);
    }

    // Render topics
    if topics.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(i18n.t("help_no_results"))
                    .color(DesignSystem::TEXT_MUTED)
                    .size(14.0)
                    .italics(),
            );
        });
    } else {
        for topic in topics {
            let title_text = if let Some(ref abbrev) = topic.abbreviation {
                format!("{} ({})", topic.title, abbrev)
            } else {
                topic.title.clone()
            };

            let header_text = egui::RichText::new(&title_text)
                .size(14.0)
                .strong()
                .color(DesignSystem::TEXT_PRIMARY);

            egui::Frame::NONE
                .fill(DesignSystem::BG_CARD)
                .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
                .corner_radius(DesignSystem::ROUNDING_MEDIUM)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.collapsing(header_text, |ui| {
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            let cat_label = i18n.category_name(&topic.category);
                            egui::Frame::NONE
                                .fill(DesignSystem::ACCENT_PRIMARY.linear_multiply(0.15))
                                .corner_radius(DesignSystem::ROUNDING_SMALL)
                                .inner_margin(egui::Margin::symmetric(8, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(cat_label)
                                            .color(DesignSystem::ACCENT_SECONDARY)
                                            .size(11.0)
                                            .strong(),
                                    );
                                });

                            ui.label(
                                egui::RichText::new(&topic.full_name)
                                    .color(DesignSystem::TEXT_SECONDARY)
                                    .size(13.0)
                                    .italics(),
                            );
                        });

                        ui.add_space(10.0);

                        ui.label(
                            egui::RichText::new(&topic.description)
                                .color(DesignSystem::TEXT_PRIMARY)
                                .size(13.0),
                        );

                        if let Some(ref example) = topic.example {
                            ui.add_space(10.0);

                            egui::Frame::NONE
                                .fill(DesignSystem::BG_INPUT)
                                .corner_radius(DesignSystem::ROUNDING_SMALL)
                                .inner_margin(8)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}{}",
                                            i18n.t("help_example_prefix"),
                                            example
                                        ))
                                        .color(DesignSystem::TEXT_SECONDARY)
                                        .size(12.0)
                                        .italics(),
                                    );
                                });
                        }
                    });
                });
            ui.add_space(12.0);
        }
    }
}

/// Renders the Shortcuts settings tab
pub fn render_shortcuts_tab(ui: &mut egui::Ui, _panel: &SettingsPanel, i18n: &I18nService) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(i18n.t("shortcuts_description"))
            .color(DesignSystem::TEXT_SECONDARY)
            .size(14.0),
    );
    ui.add_space(20.0);

    Card::new().title(i18n.t("shortcuts_title")).show(ui, |ui| {
        ui.add_space(10.0);

        let shortcuts = [
            (
                i18n.t("shortcuts_settings"),
                vec![
                    if cfg!(target_os = "macos") {
                        "⌘ Cmd"
                    } else {
                        "Ctrl"
                    },
                    ",",
                ],
            ),
            (
                i18n.t("shortcuts_shortcuts"),
                vec![
                    if cfg!(target_os = "macos") {
                        "⌘ Cmd"
                    } else {
                        "Ctrl"
                    },
                    "K",
                ],
            ),
            (i18n.t("shortcuts_help"), vec!["F1"]),
        ];

        for (label, keys) in shortcuts {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .color(DesignSystem::TEXT_PRIMARY)
                        .size(14.0),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let total_keys = keys.len();
                    for (idx, key) in keys.into_iter().rev().enumerate() {
                        render_keycap(ui, key);
                        if idx < total_keys - 1 {
                            ui.label(
                                egui::RichText::new("+")
                                    .color(DesignSystem::TEXT_MUTED)
                                    .strong()
                                    .size(14.0),
                            );
                        }
                    }
                });
            });
            ui.add_space(16.0);
        }
    });
}

/// Renders the About settings tab
pub fn render_about_tab(ui: &mut egui::Ui, _panel: &SettingsPanel, i18n: &I18nService) {
    ui.add_space(10.0);

    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new("Rustrade")
                .size(36.0)
                .strong()
                .color(DesignSystem::ACCENT_PRIMARY),
        );
        ui.label(
            egui::RichText::new(i18n.t("about_subtitle"))
                .size(14.0)
                .color(DesignSystem::TEXT_SECONDARY)
                .italics(),
        );

        ui.add_space(20.0);

        Card::new().title("").show(ui, |ui| {
            ui.label(
                egui::RichText::new(i18n.t("about_description"))
                    .color(DesignSystem::TEXT_PRIMARY)
                    .size(14.0),
            );

            ui.add_space(15.0);
            ui.separator();
            ui.add_space(15.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(i18n.t("about_platform_version"))
                        .color(DesignSystem::TEXT_SECONDARY)
                        .size(13.0),
                );
                ui.label(
                    egui::RichText::new(env!("CARGO_PKG_VERSION"))
                        .strong()
                        .color(DesignSystem::SUCCESS)
                        .size(13.0),
                );

                ui.add_space(30.0);

                ui.label(
                    egui::RichText::new(i18n.t("about_os"))
                        .color(DesignSystem::TEXT_SECONDARY)
                        .size(13.0),
                );
                ui.label(
                    egui::RichText::new(std::env::consts::OS)
                        .strong()
                        .color(DesignSystem::TEXT_PRIMARY)
                        .size(13.0),
                );
            });
        });

        ui.add_space(24.0);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(i18n.t("about_tech_stack"))
                    .size(18.0)
                    .strong()
                    .color(DesignSystem::TEXT_PRIMARY),
            );
        });
        ui.add_space(12.0);

        // Technical stack horizontal layout
        ui.horizontal(|ui| {
            let card_width = ((ui.available_width() - 32.0) / 3.0).max(50.0);
            let tech = [
                ("🦀 Rust", i18n.t("tech_rust"), i18n.t("tech_rust_desc")),
                ("🎨 egui", i18n.t("tech_egui"), i18n.t("tech_egui_desc")),
                ("⚡ Tokio", i18n.t("tech_tokio"), i18n.t("tech_tokio_desc")),
            ];

            for (name, label, desc) in tech {
                egui::Frame::NONE
                    .fill(DesignSystem::BG_CARD)
                    .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
                    .corner_radius(DesignSystem::ROUNDING_MEDIUM)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(card_width);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(name)
                                    .strong()
                                    .color(DesignSystem::ACCENT_SECONDARY)
                                    .size(16.0),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(label)
                                    .color(DesignSystem::TEXT_MUTED)
                                    .size(11.0)
                                    .italics(),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(desc)
                                    .color(DesignSystem::TEXT_SECONDARY)
                                    .size(12.0),
                            );
                        });
                    });
            }
        });
    });
}
