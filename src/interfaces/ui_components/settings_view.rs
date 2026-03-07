use super::settings_state::SettingsPanel;
use super::sidebar::{ConfigMode, SettingsTab};
use crate::application::agents::analyst::AnalystCommand;
use crate::application::client::SystemClient;
use crate::application::risk_management::commands::RiskCommand;
use crate::infrastructure::i18n::I18nService;
use crate::infrastructure::settings_persistence::{
    AnalystSettings, PersistedSettings, RiskSettings, SettingsPersistence,
};
use crate::interfaces::design_system::DesignSystem;
use crate::interfaces::settings_components;
use eframe::egui;
use tracing::error;

/// Data required for symbol selector in settings
pub struct SymbolSelectorRefs<'a> {
    pub available_symbols: &'a [String],
    pub active_symbols: &'a mut Vec<String>,
    pub symbols_loading: bool,
    pub client: &'a crate::application::client::SystemClient,
    pub state: &'a mut settings_components::SymbolSelectorState,
}

pub fn render_settings_view(
    ui: &mut egui::Ui,
    panel: &mut SettingsPanel,
    i18n: &mut I18nService,
    client: &crate::application::client::SystemClient,
    symbol_refs: Option<SymbolSelectorRefs<'_>>,
) {
    let total_height = ui.available_height();

    ui.horizontal(|ui| {
        // --- Sidebar (Left) ---
        egui::Frame::NONE
            .fill(DesignSystem::BG_PANEL)
            .inner_margin(egui::Margin::symmetric(10, 20))
            .show(ui, |ui| {
                ui.set_width(180.0);
                ui.set_min_height(total_height);
                render_settings_sidebar(ui, panel, i18n);
            });

        ui.add_space(1.0);

        // --- Content Area (Right) - allocate remaining space ---
        let content_width = ui.available_width();
        ui.allocate_ui(egui::vec2(content_width, total_height), |ui| {
            ui.vertical(|ui| {
                // Header with title and save button
                ui.horizontal(|ui| {
                    let title = match panel.active_tab {
                        SettingsTab::TradingEngine => i18n.t("settings_system_config_title"),
                        SettingsTab::Language => i18n.t("tab_language"),
                        SettingsTab::Help => i18n.t("tab_help"),
                        SettingsTab::Shortcuts => i18n.t("tab_shortcuts"),
                        SettingsTab::About => i18n.t("tab_about"),
                    };

                    ui.heading(
                        egui::RichText::new(title)
                            .size(24.0)
                            .strong()
                            .color(DesignSystem::TEXT_PRIMARY),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if panel.active_tab == SettingsTab::TradingEngine {
                            render_save_button(ui, panel, i18n, client);
                        }
                    });
                });

                ui.add_space(DesignSystem::SPACING_MEDIUM);
                ui.separator();
                ui.add_space(DesignSystem::SPACING_MEDIUM);

                // ScrollArea that fills remaining space
                let scroll_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .auto_shrink([false, false])
                    .max_height(scroll_height)
                    .show(ui, |ui| match panel.active_tab {
                        SettingsTab::TradingEngine => {
                            render_trading_engine_content(ui, panel, i18n);
                        }
                        SettingsTab::Language => {
                            settings_components::render_language_settings(ui, i18n);
                        }
                        SettingsTab::Help => {
                            settings_components::render_help_tab(ui, i18n);
                        }
                        SettingsTab::Shortcuts => {
                            settings_components::render_shortcuts_tab(ui, i18n);
                        }
                        SettingsTab::About => {
                            settings_components::render_about_tab(ui, i18n);
                        }
                    });
            });
        });
    });

    // Symbol selector rendered outside the closures to avoid borrow conflicts
    if panel.active_tab == SettingsTab::TradingEngine
        && let Some(refs) = symbol_refs
    {
        let is_crypto = std::env::var("ASSET_CLASS")
            .map(|v| v.to_lowercase() == "crypto")
            .unwrap_or(false);

        if is_crypto {
            let data = settings_components::SymbolSelectorData {
                available_symbols: refs.available_symbols,
                active_symbols: refs.active_symbols,
                symbols_loading: refs.symbols_loading,
                client: refs.client,
            };
            settings_components::render_symbol_selector(ui, data, refs.state, i18n);
        }
    }
}

/// Renders the settings sidebar navigation
fn render_settings_sidebar(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        let tabs = [
            (
                SettingsTab::TradingEngine,
                "⚙",
                i18n.t("settings_system_config_title"),
            ),
            (SettingsTab::Language, "🌐", i18n.t("tab_language")),
            (SettingsTab::Shortcuts, "⌨", i18n.t("tab_shortcuts")),
            (SettingsTab::Help, "❓", i18n.t("tab_help")),
            (SettingsTab::About, "ℹ", i18n.t("tab_about")),
        ];

        for (tab, icon, label) in tabs {
            let is_selected = panel.active_tab == tab;

            let bg = if is_selected {
                DesignSystem::ACCENT_PRIMARY.linear_multiply(0.2)
            } else {
                egui::Color32::TRANSPARENT
            };
            let text_color = if is_selected {
                DesignSystem::ACCENT_PRIMARY
            } else {
                DesignSystem::TEXT_SECONDARY
            };
            let border = if is_selected {
                egui::Stroke::new(1.0, DesignSystem::ACCENT_PRIMARY)
            } else {
                egui::Stroke::NONE
            };

            let btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{}  {}", icon, label))
                        .size(14.0)
                        .color(text_color),
                )
                .fill(bg)
                .stroke(border)
                .min_size(egui::vec2(ui.available_width(), 36.0))
                .frame(true),
            );

            if btn.clicked() {
                panel.active_tab = tab;
            }
        }
    });
}

/// Renders the Trading Engine configuration content (without symbol selector)
fn render_trading_engine_content(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.label(
        egui::RichText::new(i18n.t("settings_config_description"))
            .color(DesignSystem::TEXT_SECONDARY)
            .size(13.0),
    );
    ui.add_space(DesignSystem::SPACING_MEDIUM);

    // Mode toggle (Simple/Advanced)
    render_mode_toggle(ui, panel, i18n);

    ui.add_space(DesignSystem::SPACING_LARGE);

    if panel.config_mode == ConfigMode::Simple {
        settings_components::render_risk_settings(ui, panel, i18n);
    } else {
        settings_components::render_strategy_settings(ui, panel, i18n);
    }
}

/// Renders the Simple/Advanced mode toggle buttons
fn render_mode_toggle(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} ", i18n.t("settings_mode_label")))
                .strong()
                .size(16.0),
        );

        // Simple Mode Button
        let simple_active = panel.config_mode == ConfigMode::Simple;
        let simple_btn = egui::Button::new(
            egui::RichText::new(i18n.t("settings_mode_simple"))
                .color(if simple_active {
                    DesignSystem::TEXT_PRIMARY
                } else {
                    DesignSystem::TEXT_SECONDARY
                })
                .strong()
                .size(14.0),
        )
        .fill(if simple_active {
            DesignSystem::ACCENT_PRIMARY
        } else {
            DesignSystem::BG_CARD
        })
        .min_size(egui::vec2(120.0, 32.0));

        if ui.add(simple_btn).clicked() {
            panel.config_mode = ConfigMode::Simple;
            panel.update_from_score(panel.risk_score);
        }

        // Advanced Mode Button
        let advanced_active = panel.config_mode == ConfigMode::Advanced;
        let advanced_btn = egui::Button::new(
            egui::RichText::new(i18n.t("settings_mode_advanced"))
                .color(if advanced_active {
                    DesignSystem::TEXT_PRIMARY
                } else {
                    DesignSystem::TEXT_SECONDARY
                })
                .strong()
                .size(14.0),
        )
        .fill(if advanced_active {
            DesignSystem::ACCENT_PRIMARY
        } else {
            DesignSystem::BG_CARD
        })
        .min_size(egui::vec2(120.0, 32.0));

        if ui.add(advanced_btn).clicked() {
            panel.config_mode = ConfigMode::Advanced;
        }
    });
}

/// Renders the save button and handles configuration parsing and sending
fn render_save_button(
    ui: &mut egui::Ui,
    panel: &SettingsPanel,
    i18n: &I18nService,
    client: &SystemClient,
) {
    if ui
        .button(egui::RichText::new(i18n.t("settings_save_button")).size(18.0))
        .clicked()
    {
        // --- Save Settings to Disk ---
        let persisted_settings = PersistedSettings {
            config_mode: match panel.config_mode {
                ConfigMode::Simple => "Simple".to_string(),
                ConfigMode::Advanced => "Advanced".to_string(),
            },
            risk_score: panel.risk_score,
            risk: RiskSettings {
                max_position_size_pct: panel.max_position_size_pct.clone(),
                max_daily_loss_pct: panel.max_daily_loss_pct.clone(),
                max_drawdown_pct: panel.max_drawdown_pct.clone(),
                consecutive_loss_limit: panel.consecutive_loss_limit.clone(),
            },
            analyst: AnalystSettings {
                strategy_mode: format!("{:?}", panel.selected_strategy),
                fast_sma_period: panel.fast_sma_period.clone(),
                slow_sma_period: panel.slow_sma_period.clone(),
                rsi_period: panel.rsi_period.clone(),
                rsi_threshold: panel.rsi_threshold.clone(),
                macd_min_threshold: panel.macd_min_threshold.clone(),
                adx_threshold: panel.adx_threshold.clone(),
                min_profit_ratio: panel.min_profit_ratio.clone(),
                sma_threshold: panel.sma_threshold.clone(),
                profit_target_multiplier: panel.profit_target_multiplier.clone(),
            },
        };

        if let Ok(persistence) = SettingsPersistence::new() {
            if let Err(e) = persistence.save(&persisted_settings) {
                error!("Failed to save settings: {}", e);
            }
        } else {
            error!("Failed to initialize settings persistence for saving");
        }

        // --- Send Updates to System ---
        // Risk Config
        let risk_config = panel.to_risk_config();
        if let Err(e) = client.send_risk_command(RiskCommand::UpdateConfig(Box::new(risk_config))) {
            error!("Failed to send update config command: {}", e);
        }

        // Analyst Config
        let analyst_cfg = panel.to_analyst_config();
        if let Err(e) =
            client.send_analyst_command(AnalystCommand::UpdateConfig(Box::new(analyst_cfg)))
        {
            error!("Failed to send analyst config update: {}", e);
        }
    }
}
