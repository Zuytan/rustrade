use crate::application::agents::user_agent::UserAgent;
use eframe::egui;

impl eframe::App for UserAgent {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- 0. Enhanced Theme Configuration ---
        let mut visuals = egui::Visuals::dark();

        // Premium Dark Theme (Concept Art Style)
        visuals.window_fill = egui::Color32::from_rgb(10, 12, 16); // Very Dark Blue/Black
        visuals.panel_fill = egui::Color32::from_rgb(10, 12, 16);
        visuals.extreme_bg_color = egui::Color32::from_rgb(15, 18, 24); // Slightly lighter input bg

        // Refined Borders
        visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 44, 52));
        visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 44, 52));

        // Typography & Visibility
        visuals.widgets.noninteractive.fg_stroke =
            egui::Stroke::new(1.0, egui::Color32::from_gray(240)); // Brighter text
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(180));
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

        // Interactive Elements
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(18, 22, 29); // Card bg
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(28, 33, 42); // Button bg
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(40, 46, 58);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(66, 165, 245); // Vibrant Blue Accent

        // Selection
        visuals.selection.bg_fill = egui::Color32::from_rgba_premultiplied(66, 165, 245, 80);
        visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 181, 246));

        // Deep Shadows for "Card" effect
        visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 4],
            blur: 15,
            spread: 0,
            color: egui::Color32::from_black_alpha(160),
        };
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 8],
            blur: 20,
            spread: 0,
            color: egui::Color32::from_black_alpha(180),
        };

        ctx.set_visuals(visuals);

        // --- Keyboard Shortcuts ---
        ctx.input(|i| {
            // Ctrl/Cmd + , to go to settings
            if i.modifiers.command && i.key_pressed(egui::Key::Comma) {
                self.current_view = crate::interfaces::ui_components::DashboardView::Settings;
            }

            // F1 to open help (inside settings)
            if i.key_pressed(egui::Key::F1) {
                self.current_view = crate::interfaces::ui_components::DashboardView::Settings;
                self.settings_panel.active_tab =
                    crate::interfaces::ui_components::SettingsTab::Help;
            }

            // Ctrl/Cmd + K to open shortcuts (inside settings)
            if i.modifiers.command && i.key_pressed(egui::Key::K) {
                self.current_view = crate::interfaces::ui_components::DashboardView::Settings;
                self.settings_panel.active_tab =
                    crate::interfaces::ui_components::SettingsTab::Shortcuts;
            }
        });

        // --- 1. Process System Events (Logs & Candles) ---
        self.update();
        ctx.request_repaint(); // Ensure continuous updates for logs/charts

        // --- 2. Sidebar (Left) ---
        egui::SidePanel::left("sidebar_panel")
            .exact_width(100.0)
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(10, 12, 16))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 44, 52))),
            )
            .show(ctx, |ui| {
                crate::interfaces::ui_components::render_sidebar(
                    ui,
                    &mut self.current_view,
                    &mut self.settings_panel,
                    &self.i18n,
                );
            });

        // (Removed Right SidePanel for Settings)

        // --- 4. Central Panel ---
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(10, 12, 16))
                    .inner_margin(egui::Margin::same(24)), // Increased Margin for breathing room
            )
            .show(ctx, |ui| match self.current_view {
                crate::interfaces::ui_components::DashboardView::Dashboard => {
                    crate::interfaces::dashboard::render_dashboard(ui, self);
                }
                crate::interfaces::ui_components::DashboardView::Charts => {
                    crate::interfaces::dashboard::render_chart_panel(self, ui);
                }
                crate::interfaces::ui_components::DashboardView::Portfolio => {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new(self.i18n.t("portfolio_coming_soon"))
                                .size(20.0)
                                .weak(),
                        )
                    });
                }
                crate::interfaces::ui_components::DashboardView::Analytics => {
                    crate::interfaces::dashboard::render_analytics_view(ui, self);
                }
                crate::interfaces::ui_components::DashboardView::Settings => {
                    crate::interfaces::ui_components::render_settings_view(
                        ui,
                        &mut self.settings_panel,
                        &mut self.i18n,
                        &self.client,
                    );
                }
            });

        // Logs Panel (using extracted helper)
        crate::interfaces::dashboard::render_logs_panel(self, ctx);
    }
}

/// Configure custom fonts for the UI (Cross-platform Emoji support)
pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Priority list of emoji fonts based on OS
    let font_paths = if cfg!(target_os = "macos") {
        vec![
            "/System/Library/Fonts/Apple Color Emoji.ttc",
            "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
        ]
    } else if cfg!(target_os = "windows") {
        vec!["C:\\Windows\\Fonts\\seguiemj.ttf"]
    } else {
        // Linux / Unix candidates
        vec![
            "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/emoji/NotoColorEmoji.ttf",
            "/usr/share/fonts/TTF/NotoColorEmoji.ttf",
            "/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf",
        ]
    };

    let mut loaded = false;
    for path in font_paths {
        if let Ok(data) = std::fs::read(path) {
            fonts
                .font_data
                .insert("emoji".to_owned(), egui::FontData::from_owned(data).into());

            // Add to Proportional (default) family LAST (fallback)
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push("emoji".to_owned());
            }

            // Add to Monospace family LAST (fallback)
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                family.push("emoji".to_owned());
            }

            tracing::info!("Successfully loaded emoji font from: {}", path);
            loaded = true;
            break;
        }
    }

    if !loaded {
        tracing::warn!("Failed to load any emoji font. Icons may not render correctly.");
    }

    ctx.set_fonts(fonts);
}
