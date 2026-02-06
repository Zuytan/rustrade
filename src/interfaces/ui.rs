use crate::application::agents::user_agent::UserAgent;
use crate::interfaces::dashboard_components::{
    activity_feed::render_logs_panel, analytics_view::render_analytics_view,
    chart_panel::render_chart_panel,
};
use eframe::egui;
impl eframe::App for UserAgent {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- 0. Enhanced Theme Configuration ---
        ctx.set_visuals(crate::interfaces::design_system::DesignSystem::theme());

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
                    .fill(crate::interfaces::design_system::DesignSystem::BG_WINDOW)
                    .stroke(egui::Stroke::new(
                        1.0,
                        crate::interfaces::design_system::DesignSystem::BORDER_SUBTLE,
                    )),
            )
            .show(ctx, |ui| {
                crate::interfaces::ui_components::render_sidebar(
                    ui,
                    &mut self.current_view,
                    &mut self.settings_panel,
                    &self.i18n,
                );
            });

        // --- 4. Central Panel ---
        egui::CentralPanel::default()
            .frame(crate::interfaces::design_system::DesignSystem::main_frame())
            .show(ctx, |ui| match self.current_view {
                crate::interfaces::ui_components::DashboardView::Dashboard => {
                    crate::interfaces::dashboard::render_dashboard(ui, self);
                }
                crate::interfaces::ui_components::DashboardView::Charts => {
                    render_chart_panel(self, ui);
                }

                crate::interfaces::ui_components::DashboardView::Analytics => {
                    render_analytics_view(ui, self);
                }
                crate::interfaces::ui_components::DashboardView::Settings => {
                    // Check if crypto mode for symbol selector
                    let is_crypto = std::env::var("ASSET_CLASS")
                        .map(|v| v.to_lowercase() == "crypto")
                        .unwrap_or(false);

                    let symbol_refs = if is_crypto {
                        Some(crate::interfaces::ui_components::SymbolSelectorRefs {
                            available_symbols: &self.available_symbols,
                            active_symbols: &mut self.active_symbols,
                            symbols_loading: self.symbols_loading,
                            client: &self.client,
                            state: &mut self.symbol_selector_state,
                        })
                    } else {
                        None
                    };

                    crate::interfaces::ui_components::render_settings_view(
                        ui,
                        &mut self.settings_panel,
                        &mut self.i18n,
                        &self.client,
                        symbol_refs,
                    );
                }
            });

        // Logs Panel (using extracted helper)
        render_logs_panel(self, ctx);
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
