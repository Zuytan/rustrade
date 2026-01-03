use crate::domain::ui::I18nService;
use eframe::egui;

/// Settings tab enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Language,
    Help,
    Shortcuts,
    About,
}

/// Dashboard View enumeration for Sidebar Navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardView {
    Dashboard,
    Charts,
    Portfolio,
    Settings,
}

impl DashboardView {
    pub fn icon(&self) -> &'static str {
        match self {
            DashboardView::Dashboard => "📊",
            DashboardView::Charts => "📈",
            DashboardView::Portfolio => "💼",
            DashboardView::Settings => "⚙️",
        }
    }

        pub fn label(&self, i18n: &I18nService) -> String {
        match self {
            DashboardView::Dashboard => i18n.t("nav_dashboard").to_string(),
            DashboardView::Charts => i18n.t("nav_charts").to_string(),
            DashboardView::Portfolio => i18n.t("nav_portfolio").to_string(),
            DashboardView::Settings => i18n.t("nav_settings").to_string(),
        }
    }
}

pub fn render_sidebar(
    ui: &mut egui::Ui,
    current_view: &mut DashboardView,
    _settings_panel: &mut SettingsPanel, // Kept for signature compatibility but unused
    i18n: &I18nService,
) {
    ui.add_space(20.0);
    
    // Logo / App Name (Custom Painted)
    ui.vertical_centered(|ui| {
        let (rect, _response) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        
        // Draw Logo Circle with subtle gradient feel
        ui.painter().circle_filled(
            rect.center(), 
            18.0, 
            egui::Color32::from_rgb(22, 27, 34) // Darker bg
        );
        ui.painter().circle_stroke(
            rect.center(), 
            18.0, 
            egui::Stroke::new(1.5, egui::Color32::from_rgb(88, 166, 255)) // Blue Ring
        );
        
        // Draw "R"
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "R",
            egui::FontId::proportional(22.0),
            egui::Color32::WHITE,
        );

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(i18n.t("app_name"))
                .size(11.0)
                .strong()
                .color(egui::Color32::from_rgb(205, 217, 229)),
        );
    });

    ui.add_space(25.0);
    ui.separator();
    ui.add_space(20.0);

    // Navigation Items (Including Settings at the bottom implicitly, or we can explicit it)
    // Let's put Settings at the end of the list
    let views = [
        DashboardView::Dashboard,
        DashboardView::Charts,
        DashboardView::Portfolio,
        DashboardView::Settings,
    ];

    for view in views {
        let is_selected = *current_view == view;
        let icon = view.icon();
        let label = view.label(i18n);

        let item_height = 64.0;
        let h_margin = 12.0;
        let available_width = ui.available_width() - (h_margin * 2.0);
        
        // Centered allocation
        ui.horizontal(|ui| {
            ui.add_space(h_margin);
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(available_width, item_height), 
                egui::Sense::click()
            );

            if response.clicked() {
                *current_view = view;
            }

            // --- BACKGROUND / FRAME PAINTING ---
            let frame = if is_selected {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(22, 27, 34))
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(41, 121, 255))) // Blue Active Stroke
                    .shadow(egui::epaint::Shadow {
                        offset: [0.0, 4.0].into(),
                        blur: 15.0,
                        spread: 0.0,
                        color: egui::Color32::from_rgba_premultiplied(41, 121, 255, 35), // Blue Glow
                    })
                    .inner_margin(egui::Margin::symmetric(0.0, 8.0))
            } else if response.hovered() {
                egui::Frame::none()
                    .fill(egui::Color32::from_white_alpha(5))
                    .rounding(12.0)
                    .inner_margin(egui::Margin::symmetric(0.0, 8.0))
            } else {
                egui::Frame::none()
                    .inner_margin(egui::Margin::symmetric(0.0, 8.0))
            };

            // Render content inside the frame
            // Create a new UI at the desired position
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child_ui.vertical(|ui| {
                frame.show(ui, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(6.0);
                            
                            let text_color = if is_selected {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::from_gray(140)
                            };
                            
                            let icon_color = if is_selected { 
                                egui::Color32::from_rgb(88, 166, 255) 
                            } else { 
                                text_color 
                            };

                            ui.label(egui::RichText::new(icon).size(24.0).color(icon_color));
                            ui.add_space(2.0);
                            ui.label(egui::RichText::new(&label).size(11.0).color(text_color));
                            ui.add_space(6.0);
                        });
                    });
                });
            });
            
            // Global interaction over the whole rect (including text)
            if ui.interact(rect, response.id, egui::Sense::click()).clicked() {
                *current_view = view;
            }
        });

        ui.add_space(12.0);
    }
}


impl SettingsTab {
    /// Get all available tabs
    pub fn all() -> Vec<Self> {
        vec![
            SettingsTab::Language,
            SettingsTab::Help,
            SettingsTab::Shortcuts,
            SettingsTab::About,
        ]
    }

    /// Get the icon and label for this tab
    pub fn icon_and_label(&self, i18n: &I18nService) -> String {
        match self {
            SettingsTab::Language => format!("🌐 {}", i18n.t("tab_language")),
            SettingsTab::Help => format!("❓ {}", i18n.t("tab_help")),
            SettingsTab::Shortcuts => format!("⌨️ {}", i18n.t("tab_shortcuts")),
            SettingsTab::About => format!("ℹ️ {}", i18n.t("tab_about")),
        }
    }
}

/// Settings panel state
pub struct SettingsPanel {
    pub is_open: bool,
    pub active_tab: SettingsTab,
}

impl SettingsPanel {
    /// Create a new settings panel (closed by default)
    pub fn new() -> Self {
        Self {
            is_open: false,
            active_tab: SettingsTab::Language,
        }
    }

    /// Toggle the panel open/closed
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
    }

    /// Open the panel with a specific tab
    pub fn open_with_tab(&mut self, tab: SettingsTab) {
        self.is_open = true;
        self.active_tab = tab;
    }
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// Render the language selection tab
pub fn render_language_tab(ui: &mut egui::Ui, i18n: &mut I18nService) {
    ui.add_space(8.0);
    ui.heading(
        egui::RichText::new(i18n.t("tab_language"))
            .size(16.0)
            .strong(),
    );
    ui.add_space(8.0);

    ui.label(
        egui::RichText::new(i18n.t("language_description"))
            .size(12.0)
            .color(egui::Color32::from_gray(170)),
    );

    ui.add_space(12.0);

    let languages = i18n.available_languages().to_vec();
    let current_code = i18n.current_language_code().to_string();

    ui.vertical(|ui| {
        for lang in languages {
            ui.push_id(&lang.code, |ui| {
                let is_selected = current_code == lang.code;

                // Interactive Card
                let button_response = ui
                    .scope(|ui| {
                        egui::Frame::none()
                            .fill(if is_selected {
                                egui::Color32::from_rgb(33, 38, 45).linear_multiply(1.5) // Slightly brighter selected
                            } else {
                                egui::Color32::from_rgb(22, 27, 34)
                            })
                            .inner_margin(egui::Margin::same(10.0))
                            .rounding(4.0)
                            .stroke(egui::Stroke::new(
                                1.0,
                                if is_selected {
                                    egui::Color32::from_rgb(88, 166, 255)
                                } else {
                                    egui::Color32::from_rgb(48, 54, 61)
                                },
                            ))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Flag
                                    ui.label(egui::RichText::new(&lang.flag).size(20.0)); // Reduced size
                                    ui.add_space(8.0);
                                    
                                    // Text Info
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(&lang.name)
                                                    .size(14.0)
                                                    .strong()
                                                    .color(if is_selected {
                                                        egui::Color32::WHITE
                                                    } else {
                                                        egui::Color32::from_gray(220)
                                                    }),
                                            );
                                            
                                            // Native name in parens
                                            ui.label(
                                                egui::RichText::new(format!("({})", lang.native_name))
                                                    .size(12.0)
                                                    .color(egui::Color32::from_gray(140)),
                                            );
                                        });
                                        
                                        ui.label(
                                            egui::RichText::new(lang.code.to_uppercase())
                                                .size(10.0)
                                                .color(egui::Color32::from_gray(120)),
                                        );
                                    });

                                    // Checkmark
                                    if is_selected {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new("✓")
                                                        .size(16.0)
                                                        .color(egui::Color32::from_rgb(88, 166, 255)),
                                                );
                                            },
                                        );
                                    }
                                });
                            })
                            .response
                    })
                    .inner;

                // Handle interactions
                let button_response = ui.interact(
                    button_response.rect,
                    button_response.id.with("click"),
                    egui::Sense::click(),
                );

                if button_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                if button_response.clicked() {
                    i18n.set_language(&lang.code);
                }
            });
            ui.add_space(6.0);
        }
    });
}

/// Render the help tab
pub fn render_help_tab(ui: &mut egui::Ui, i18n: &I18nService) {
    ui.add_space(10.0);
    ui.heading(egui::RichText::new(i18n.t("tab_help")).size(16.0));
    ui.add_space(10.0);

    egui::ScrollArea::vertical()
        .id_salt("help_topics_scroll")
        .show(ui, |ui| {
            ui.add_space(4.0);
            for topic in i18n.help_topics() {
                ui.push_id(&topic.id, |ui| {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(22, 27, 34))
                        .inner_margin(egui::Margin::same(12.0))
                        .rounding(6.0)
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                        .show(ui, |ui| {
                            // Title
                            ui.label(
                                egui::RichText::new(&topic.title)
                                    .size(14.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(88, 166, 255)),
                            );

                            // Full name
                            ui.label(
                                egui::RichText::new(&topic.full_name)
                                    .size(12.0)
                                    .color(egui::Color32::from_gray(180)),
                            );

                            ui.add_space(6.0);

                            // Description
                            ui.label(
                                egui::RichText::new(&topic.description)
                                    .size(11.0)
                                    .line_height(Some(16.0))
                                    .color(egui::Color32::from_gray(200)),
                            );

                            // Example if available
                            if let Some(example) = &topic.example {
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(format!("💡 {}", example))
                                        .size(10.0)
                                        .italics()
                                        .color(egui::Color32::from_gray(150)),
                                );
                            }
                        });
                });
                ui.add_space(8.0);
            }
        });
}

/// Render the keyboard shortcuts tab
pub fn render_shortcuts_tab(ui: &mut egui::Ui, i18n: &I18nService) {
    ui.add_space(10.0);
    ui.heading(egui::RichText::new(i18n.t("tab_shortcuts")).size(16.0));
    ui.add_space(10.0);

    ui.label(
        egui::RichText::new(i18n.t("shortcuts_description"))
            .size(12.0)
            .color(egui::Color32::from_gray(180)),
    );

    ui.add_space(15.0);

    // Define shortcuts
    let shortcuts = vec![
        ("shortcuts_settings", if cfg!(target_os = "macos") { "⌘ ," } else { "Ctrl + ," }),
        ("shortcuts_help", "F1"),
        ("shortcuts_shortcuts", if cfg!(target_os = "macos") { "⌘ K" } else { "Ctrl + K" }),
    ];

    ui.vertical(|ui| {
        for (key, shortcut) in shortcuts {
            ui.push_id(key, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(22, 27, 34))
                    .inner_margin(egui::Margin::same(10.0))
                    .rounding(6.0)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Shortcut keys
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(33, 38, 45))
                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                .rounding(4.0)
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 45, 50)))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(shortcut)
                                            .size(11.0)
                                            .family(egui::FontFamily::Monospace)
                                            .color(egui::Color32::from_rgb(255, 212, 59)),
                                    );
                                });

                            ui.add_space(10.0);

                            // Description
                            ui.label(
                                egui::RichText::new(i18n.t(key))
                                    .size(12.0)
                                    .color(egui::Color32::from_gray(200)),
                            );
                        });
                    });
            });
            ui.add_space(6.0);
        }
    });
}

/// Render the about tab
pub fn render_about_tab(ui: &mut egui::Ui, i18n: &I18nService) {
    ui.add_space(10.0);
    ui.heading(egui::RichText::new(i18n.t("tab_about")).size(16.0));
    ui.add_space(10.0);

    ui.vertical_centered(|ui| {
        ui.add_space(20.0);

        // Logo/Icon
        ui.label(egui::RichText::new("📊").size(48.0));
        ui.add_space(10.0);

        // App name
        ui.label(
            egui::RichText::new(i18n.t("app_name"))
                .size(24.0)
                .strong()
                .color(egui::Color32::from_rgb(88, 166, 255)),
        );

        ui.add_space(5.0);

        // Version
        ui.label(
            egui::RichText::new(i18n.tf("version_label", &[("version", env!("CARGO_PKG_VERSION"))]))
                .size(12.0)
                .color(egui::Color32::from_gray(160)),
        );

        ui.add_space(20.0);

        // Description
        ui.label(
            egui::RichText::new(i18n.t("about_description"))
                .size(12.0)
                .color(egui::Color32::from_gray(200)),
        );

        ui.add_space(30.0);

        // Built with section
        ui.separator();
        ui.add_space(10.0);

        ui.label(
            egui::RichText::new(i18n.t("built_with"))
                .size(11.0)
                .color(egui::Color32::from_gray(150)),
        );

        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("🦀").size(14.0));
            ui.label(
                egui::RichText::new(i18n.t("tech_rust"))
                    .size(11.0)
                    .color(egui::Color32::from_gray(180)),
            );
            ui.label(egui::RichText::new("•").color(egui::Color32::from_gray(100)));
            ui.label(egui::RichText::new("🎨").size(14.0)); // Theme/UI
            ui.label(
                egui::RichText::new(i18n.t("tech_egui"))
                    .size(11.0)
                    .color(egui::Color32::from_gray(180)),
            );
            ui.label(egui::RichText::new("•").color(egui::Color32::from_gray(100)));
            ui.label(egui::RichText::new("⚡").size(14.0)); // High Voltage (Speed)
            ui.label(
                egui::RichText::new(i18n.t("tech_tokio"))
                    .size(11.0)
                    .color(egui::Color32::from_gray(180)),
            );
        });
    });

}

/// Render the Settings View (Central Panel)
pub fn render_settings_view(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &mut I18nService) {
    ui.vertical(|ui| {
        ui.add_space(10.0);

        // Header
        ui.heading(
            egui::RichText::new(format!(
                "⚙️ {}",
                i18n.t("settings_title")
            ))
            .size(24.0)
            .strong(),
        );

        ui.add_space(20.0);
        ui.separator();
        ui.add_space(20.0);

        // Tab buttons (Larger for main view)
        ui.horizontal(|ui| {
            for tab in SettingsTab::all() {
                let is_active = panel.active_tab == tab;
                let button = egui::Button::new(
                    egui::RichText::new(tab.icon_and_label(i18n)).size(14.0),
                )
                .min_size(egui::vec2(100.0, 30.0))
                .fill(if is_active {
                    egui::Color32::from_rgb(56, 139, 253)
                } else {
                    egui::Color32::from_rgb(33, 38, 45)
                });

                if ui.add(button).clicked() {
                    panel.active_tab = tab;
                }
            }
        });

        ui.add_space(30.0);

        // Content (Wrapped in a frame)
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(22, 27, 34))
            .rounding(8.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
            .inner_margin(20.0)
            .show(ui, |ui| {
                ui.set_min_height(400.0);
                ui.set_width(ui.available_width());
                
                match panel.active_tab {
                    SettingsTab::Language => {
                        render_language_tab(ui, i18n);
                    }
                    SettingsTab::Help => {
                        render_help_tab(ui, i18n);
                    }
                    SettingsTab::Shortcuts => {
                        render_shortcuts_tab(ui, i18n);
                    }
                    SettingsTab::About => {
                        render_about_tab(ui, i18n);
                    }
                }
            });
    });
}
