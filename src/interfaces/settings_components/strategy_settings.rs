//! Strategy settings component (Advanced Mode)

use crate::infrastructure::i18n::I18nService;
use crate::interfaces::components::card::Card;
use crate::interfaces::design_system::DesignSystem;
use crate::interfaces::ui_components::SettingsPanel;
use eframe::egui;

/// Helper to render a setting row with a label, input field, and tooltip hint
fn ui_setting_with_hint(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        // Larger text for labels to fill space better
        let _label_response = ui.label(
            egui::RichText::new(label)
                .size(14.0)
                .color(DesignSystem::TEXT_PRIMARY),
        );

        // Add a (?) hint icon
        ui.label(
            egui::RichText::new("(?)")
                .weak()
                .size(12.0)
                .color(DesignSystem::TEXT_MUTED),
        )
        .on_hover_text(hint);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Styled Input Field
            let response = ui.add(
                egui::TextEdit::singleline(value)
                    .font(egui::FontId::proportional(14.0))
                    .desired_width(120.0)
                    .min_size(egui::vec2(120.0, 32.0))
                    .vertical_align(egui::Align::Center)
                    .frame(false), // Custom frame
            );

            // Draw custom frame around text edit
            let rect = response.rect.expand(4.0);
            let stroke_color = if response.has_focus() {
                DesignSystem::BORDER_FOCUS
            } else {
                DesignSystem::BORDER_SUBTLE
            };
            ui.painter().rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0, stroke_color),
                egui::StrokeKind::Outside,
            );
            ui.painter().rect_filled(rect, 4.0, DesignSystem::BG_INPUT);
        });
    });
    // Add significant vertical spacing between rows
    ui.add_space(20.0);
}

/// Renders the Advanced Mode strategy settings
pub fn render_strategy_settings(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.add_space(20.0); // Space at top

    // --- Risk Management Group ---
    // --- Risk Management Group ---
    Card::new()
        .title(i18n.t("settings_group_risk"))
        .show(ui, |ui| {
            ui.add_space(15.0);

            ui_setting_with_hint(
                ui,
                i18n.t("settings_risk_max_pos"),
                &mut panel.max_position_size_pct,
                i18n.t("settings_risk_max_pos_hint"),
            );

            ui_setting_with_hint(
                ui,
                i18n.t("settings_risk_max_loss"),
                &mut panel.max_daily_loss_pct,
                i18n.t("settings_risk_max_loss_hint"),
            );

            ui_setting_with_hint(
                ui,
                i18n.t("settings_risk_max_dd"),
                &mut panel.max_drawdown_pct,
                i18n.t("settings_risk_max_dd_hint"),
            );

            ui_setting_with_hint(
                ui,
                i18n.t("settings_risk_consecutive_loss"),
                &mut panel.consecutive_loss_limit,
                i18n.t("settings_risk_consecutive_loss_hint"),
            );
        });

    ui.add_space(40.0); // More space between groups

    // --- Strategy Group ---
    // --- Strategy Group ---
    Card::new()
        .title(i18n.t("settings_group_strategy"))
        .show(ui, |ui| {
            ui.add_space(15.0);

            ui.collapsing(i18n.t("settings_subgroup_trend"), |ui| {
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_fast_sma"),
                    &mut panel.fast_sma_period,
                    i18n.t("settings_strat_fast_sma_hint"),
                );
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_slow_sma"),
                    &mut panel.slow_sma_period,
                    i18n.t("settings_strat_slow_sma_hint"),
                );
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_sma_thresh"),
                    &mut panel.sma_threshold,
                    i18n.t("settings_strat_sma_thresh_hint"),
                );
            });

            ui.collapsing(i18n.t("settings_subgroup_oscillators"), |ui| {
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_rsi_period"),
                    &mut panel.rsi_period,
                    i18n.t("settings_strat_rsi_period_hint"),
                );
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_rsi_thresh"),
                    &mut panel.rsi_threshold,
                    i18n.t("settings_strat_rsi_thresh_hint"),
                );
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_macd_min"),
                    &mut panel.macd_min_threshold,
                    i18n.t("settings_strat_macd_min_hint"),
                );
            });

            ui.collapsing(i18n.t("settings_subgroup_advanced"), |ui| {
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_adx_thresh"),
                    &mut panel.adx_threshold,
                    i18n.t("settings_strat_adx_thresh_hint"),
                );
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_min_rr"),
                    &mut panel.min_profit_ratio,
                    i18n.t("settings_strat_min_rr_hint"),
                );
                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_strat_profit_mult"),
                    &mut panel.profit_target_multiplier,
                    i18n.t("settings_strat_profit_mult_hint"),
                );
            });
        });
}
