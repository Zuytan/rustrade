//! Symbol Selector Component for dynamic crypto pair selection
//!
//! Provides a searchable, multi-select interface for choosing which
//! crypto pairs to trade from the available exchange symbols.

use crate::application::agents::sentinel::SentinelCommand;
use crate::application::client::SystemClient;
use crate::infrastructure::i18n::I18nService;
use crate::interfaces::components::card::Card;
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// State for the symbol selector UI
#[derive(Default)]
pub struct SymbolSelectorState {
    /// Search query for filtering symbols
    pub search_query: String,
    /// Currently selected symbols in the UI (pending apply)
    pub pending_selection: Vec<String>,
    /// Whether the selector has been initialized
    pub initialized: bool,
}

impl SymbolSelectorState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize with current active symbols
    pub fn initialize(&mut self, current_symbols: &[String]) {
        if !self.initialized {
            self.pending_selection = current_symbols.to_vec();
            self.initialized = true;
        }
    }
}

/// Data container for symbol selector rendering
pub struct SymbolSelectorData<'a> {
    pub available_symbols: &'a [String],
    pub active_symbols: &'a mut Vec<String>,
    pub symbols_loading: bool,
    pub client: &'a SystemClient,
}

/// Renders the symbol selector component
pub fn render_symbol_selector(
    ui: &mut egui::Ui,
    data: SymbolSelectorData<'_>,
    state: &mut SymbolSelectorState,
    i18n: &I18nService,
) {
    // Initialize state if needed
    state.initialize(data.active_symbols);

    Card::new()
        .title(i18n.t("settings_symbol_selector_title"))
        .show(ui, |ui| {
            ui.add_space(8.0);

            // Loading indicator
            if data.symbols_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new(i18n.t("loading_symbols"))
                            .color(DesignSystem::TEXT_SECONDARY),
                    );
                });
                return;
            }

            // Stats row
            ui.horizontal(|ui| {
                let selected_count = state.pending_selection.len();
                let total_count = data.available_symbols.len();

                ui.label(
                    egui::RichText::new(format!(
                        "{} {} / {} {}",
                        selected_count,
                        i18n.t("selected"),
                        total_count,
                        i18n.t("available")
                    ))
                    .color(DesignSystem::TEXT_SECONDARY)
                    .size(13.0),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Clear button
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(i18n.t("clear_all"))
                                    .size(12.0)
                                    .color(DesignSystem::TEXT_SECONDARY),
                            )
                            .fill(DesignSystem::BG_INPUT),
                        )
                        .clicked()
                    {
                        state.pending_selection.clear();
                    }

                    // Select top 10 button
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(i18n.t("select_top_10"))
                                    .size(12.0)
                                    .color(DesignSystem::TEXT_SECONDARY),
                            )
                            .fill(DesignSystem::BG_INPUT),
                        )
                        .clicked()
                    {
                        state.pending_selection =
                            data.available_symbols.iter().take(10).cloned().collect();
                    }
                });
            });

            ui.add_space(12.0);

            // Search input
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🔍")
                        .size(16.0)
                        .color(DesignSystem::TEXT_MUTED),
                );

                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.search_query)
                        .hint_text(i18n.t("search_symbols_hint"))
                        .desired_width(ui.available_width() - 40.0)
                        .font(egui::FontId::proportional(14.0)),
                );

                // Clear search button
                if !state.search_query.is_empty() && ui.button("✕").clicked() {
                    state.search_query.clear();
                    response.request_focus();
                }
            });

            ui.add_space(12.0);

            // Filter symbols based on search
            let search_lower = state.search_query.to_lowercase();
            let filtered_symbols: Vec<&String> = data
                .available_symbols
                .iter()
                .filter(|s| search_lower.is_empty() || s.to_lowercase().contains(&search_lower))
                .collect();

            // Symbol list with scroll area
            let scroll_height = 250.0;
            egui::ScrollArea::vertical()
                .id_salt("symbol_list")
                .max_height(scroll_height)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    // Grid layout for symbols (3 columns)
                    egui::Grid::new("symbol_grid")
                        .num_columns(3)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            for (idx, symbol) in filtered_symbols.iter().enumerate() {
                                let is_selected = state.pending_selection.contains(symbol);

                                let bg_color = if is_selected {
                                    DesignSystem::ACCENT_PRIMARY.linear_multiply(0.2)
                                } else {
                                    DesignSystem::BG_INPUT
                                };

                                let text_color = if is_selected {
                                    DesignSystem::ACCENT_PRIMARY
                                } else {
                                    DesignSystem::TEXT_PRIMARY
                                };

                                let btn = egui::Button::new(
                                    egui::RichText::new(symbol.as_str())
                                        .size(12.0)
                                        .color(text_color),
                                )
                                .fill(bg_color)
                                .min_size(egui::vec2(90.0, 28.0));

                                if ui.add(btn).clicked() {
                                    if is_selected {
                                        state.pending_selection.retain(|s| s != *symbol);
                                    } else {
                                        state.pending_selection.push((*symbol).clone());
                                    }
                                }

                                // New row every 3 items
                                if (idx + 1) % 3 == 0 {
                                    ui.end_row();
                                }
                            }
                        });

                    // Show message if no results
                    if filtered_symbols.is_empty() {
                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new(i18n.t("no_symbols_found"))
                                .color(DesignSystem::TEXT_MUTED)
                                .italics(),
                        );
                    }
                });

            ui.add_space(16.0);

            // Apply button
            let has_changes = state.pending_selection != *data.active_symbols;
            let can_apply = !state.pending_selection.is_empty() && has_changes;

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let apply_btn = egui::Button::new(
                        egui::RichText::new(format!("✓ {}", i18n.t("apply_symbols")))
                            .size(14.0)
                            .strong()
                            .color(if can_apply {
                                DesignSystem::TEXT_PRIMARY
                            } else {
                                DesignSystem::TEXT_MUTED
                            }),
                    )
                    .fill(if can_apply {
                        DesignSystem::ACCENT_PRIMARY
                    } else {
                        DesignSystem::BG_INPUT
                    })
                    .min_size(egui::vec2(140.0, 36.0));

                    if ui.add_enabled(can_apply, apply_btn).clicked() {
                        // Send command to Sentinel to update subscriptions
                        let new_symbols = state.pending_selection.clone();
                        if let Err(e) =
                            data.client
                                .send_sentinel_command(SentinelCommand::UpdateSymbols(
                                    new_symbols.clone(),
                                ))
                        {
                            tracing::error!("Failed to update symbols: {}", e);
                        } else {
                            // Update active symbols
                            *data.active_symbols = new_symbols;
                            tracing::info!("Symbol selection updated: {:?}", data.active_symbols);
                        }
                    }

                    // Reset button
                    if has_changes
                        && ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(i18n.t("reset"))
                                        .size(12.0)
                                        .color(DesignSystem::TEXT_SECONDARY),
                                )
                                .fill(DesignSystem::BG_INPUT),
                            )
                            .clicked()
                    {
                        state.pending_selection = data.active_symbols.clone();
                    }
                });
            });
        });
}
