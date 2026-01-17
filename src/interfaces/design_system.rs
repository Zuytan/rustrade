use eframe::egui;

/// Premium Dark Mode Design System
pub struct DesignSystem;

impl DesignSystem {
    // --- Colors ---

    // Backgrounds
    pub const BG_WINDOW: egui::Color32 = egui::Color32::from_rgb(10, 12, 16); // #0A0C10
    pub const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(10, 12, 16); // #0A0C10
    pub const BG_CARD: egui::Color32 = egui::Color32::from_rgb(22, 27, 34); // #161B22
    pub const BG_CARD_HOVER: egui::Color32 = egui::Color32::from_rgb(28, 33, 40);
    pub const BG_INPUT: egui::Color32 = egui::Color32::from_rgb(15, 18, 24);

    // Accents
    pub const ACCENT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(41, 121, 255); // #2979FF (Blue)
    pub const ACCENT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(66, 165, 245); // Lighter Blue

    // Status
    pub const SUCCESS: egui::Color32 = egui::Color32::from_rgb(0, 230, 118); // #00E676
    pub const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 23, 68); // #FF1744
    pub const WARNING: egui::Color32 = egui::Color32::from_rgb(255, 145, 0); // #FF9100
    pub const INFO: egui::Color32 = egui::Color32::from_rgb(41, 121, 255);

    // Text
    pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(240, 246, 252);
    pub const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_gray(160);
    pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_gray(100);

    // Borders
    pub const BORDER_SUBTLE: egui::Color32 = egui::Color32::from_rgb(48, 54, 61);
    pub const BORDER_FOCUS: egui::Color32 = egui::Color32::from_rgb(56, 139, 253);

    // --- Metrics ---

    pub const ROUNDING_SMALL: f32 = 4.0;
    pub const ROUNDING_MEDIUM: f32 = 8.0;
    pub const ROUNDING_LARGE: f32 = 12.0;

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

        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, Self::TEXT_SECONDARY);
        visuals.widgets.inactive.weak_bg_fill = Self::BG_CARD;
        visuals.widgets.inactive.bg_fill = Self::BG_CARD;

        visuals.widgets.hovered.bg_fill = Self::BG_CARD_HOVER;
        visuals.widgets.active.bg_fill = Self::ACCENT_SECONDARY;

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
    }

    /// Application Main Layout Frame
    pub fn main_frame() -> egui::Frame {
        egui::Frame::NONE
            .fill(Self::BG_WINDOW)
            .inner_margin(egui::Margin::same(Self::SPACING_LARGE as i8))
    }
}
