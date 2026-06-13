use eframe::egui;

/// Premium Dark Mode Design System
pub struct DesignSystem;

impl DesignSystem {
    // --- Colors ---

    // Backgrounds
    pub const BG_WINDOW: egui::Color32 = egui::Color32::from_rgb(8, 9, 13); // #08090D (Deep rich dark background)
    pub const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(11, 13, 20); // #0B0D14 (Slightly lighter sidebars/panels)
    pub const BG_CARD: egui::Color32 = egui::Color32::from_rgb(18, 22, 33); // #121621 (Clean dark slate-blue card)
    pub const BG_CARD_HOVER: egui::Color32 = egui::Color32::from_rgb(24, 29, 43); // #181D2B
    pub const BG_INPUT: egui::Color32 = egui::Color32::from_rgb(13, 16, 25); // #0D1019

    // Accents
    pub const ACCENT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(99, 102, 241); // #6366F1 (Electric Indigo)
    pub const ACCENT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(129, 140, 248); // #818CF8 (Soft Indigo)

    // Status
    pub const SUCCESS: egui::Color32 = egui::Color32::from_rgb(16, 185, 129); // #10B981 (Emerald Green)
    pub const DANGER: egui::Color32 = egui::Color32::from_rgb(239, 68, 68); // #EF4444 (Vibrant Coral Red)
    pub const WARNING: egui::Color32 = egui::Color32::from_rgb(245, 158, 11); // #F59E0B (Amber Gold)
    pub const INFO: egui::Color32 = egui::Color32::from_rgb(59, 130, 246); // #3B82F6 (Blue)

    // Text
    pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(243, 244, 246); // #F3F4F6
    pub const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(156, 163, 175); // #9CA3AF
    pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(107, 114, 128); // #6B7280

    // Borders
    pub const BORDER_SUBTLE: egui::Color32 = egui::Color32::from_rgb(31, 41, 55); // #1F2937 (Modern charcoal/navy border)
    pub const BORDER_FOCUS: egui::Color32 = egui::Color32::from_rgb(99, 102, 241); // #6366F1 (Indigo Focus)

    // --- Metrics ---

    pub const ROUNDING_SMALL: f32 = 6.0;
    pub const ROUNDING_MEDIUM: f32 = 12.0;
    pub const ROUNDING_LARGE: f32 = 18.0;

    pub const SPACING_SMALL: f32 = 8.0;
    pub const SPACING_MEDIUM: f32 = 16.0;
    pub const SPACING_LARGE: f32 = 24.0;

    // --- Styles ---

    /// Returns the standard visual style for the application
    pub fn theme() -> egui::Visuals {
        let mut visuals = egui::Visuals::dark();

        visuals.window_fill = Self::BG_WINDOW;
        visuals.panel_fill = Self::BG_PANEL;
        visuals.extreme_bg_color = Self::BG_INPUT;

        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, Self::BORDER_SUBTLE);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, Self::TEXT_PRIMARY);
        visuals.widgets.noninteractive.corner_radius = Self::ROUNDING_SMALL.into();

        // Inactive widgets (buttons, inactive handles, slider tracks)
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, Self::TEXT_SECONDARY);
        visuals.widgets.inactive.weak_bg_fill = Self::BG_INPUT; // Dark inset track background
        visuals.widgets.inactive.bg_fill = Self::BG_CARD_HOVER; // Lighter distinct background for handles
        visuals.widgets.inactive.corner_radius = Self::ROUNDING_SMALL.into();

        // Hovered widgets
        visuals.widgets.hovered.bg_fill = Self::ACCENT_PRIMARY; // Vibrant primary accent on hover
        visuals.widgets.hovered.weak_bg_fill = Self::BG_INPUT; // Keep track visible
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, Self::TEXT_PRIMARY);
        visuals.widgets.hovered.corner_radius = Self::ROUNDING_SMALL.into();

        // Active widgets (being clicked or dragged)
        visuals.widgets.active.bg_fill = Self::ACCENT_SECONDARY;
        visuals.widgets.active.weak_bg_fill = Self::BG_INPUT; // Keep track visible
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, Self::TEXT_PRIMARY); // Higher thickness and brightness for cursor
        visuals.widgets.active.corner_radius = Self::ROUNDING_SMALL.into();

        visuals.selection.bg_fill = Self::ACCENT_PRIMARY.linear_multiply(0.3);
        visuals.selection.stroke = egui::Stroke::new(1.0, Self::ACCENT_PRIMARY);

        visuals
    }

    /// Standard Card Styling
    pub fn card_frame() -> egui::Frame {
        egui::Frame::NONE
            .fill(Self::BG_CARD)
            .corner_radius(Self::ROUNDING_MEDIUM)
            .stroke(egui::Stroke::new(1.0, Self::BORDER_SUBTLE))
            .inner_margin(Self::SPACING_MEDIUM as i8)
            .shadow(egui::epaint::Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 80),
            })
    }

    /// Application Main Layout Frame
    pub fn main_frame() -> egui::Frame {
        egui::Frame::NONE
            .fill(Self::BG_WINDOW)
            .inner_margin(egui::Margin::same(Self::SPACING_LARGE as i8))
    }
}
