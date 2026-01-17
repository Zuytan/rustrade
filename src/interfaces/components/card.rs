use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// A generic card container with standard styling
pub struct Card {
    title: Option<String>,
    min_height: f32,
    active: bool,
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Card {
    pub fn new() -> Self {
        Self {
            title: None,
            min_height: 0.0,
            active: false,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn min_height(mut self, height: f32) -> Self {
        self.min_height = height;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let mut frame = DesignSystem::card_frame();

        if self.active {
            frame = frame
                .stroke(egui::Stroke::new(1.5, DesignSystem::ACCENT_PRIMARY))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 15,
                    spread: 0,
                    color: DesignSystem::ACCENT_PRIMARY.linear_multiply(0.15),
                });
        }

        frame.show(ui, |ui| {
            // Don't set min_width as it causes issues inside ScrollArea
            if self.min_height > 0.0 {
                ui.set_min_height(self.min_height);
            }

            if let Some(title) = self.title {
                ui.label(
                    egui::RichText::new(title)
                        .size(12.0)
                        .color(DesignSystem::TEXT_SECONDARY)
                        .strong(),
                );
                ui.add_space(DesignSystem::SPACING_SMALL);
            }

            add_contents(ui)
        })
    }
}
