//! Strategy settings component (Advanced Mode)

use crate::infrastructure::i18n::I18nService;
use crate::interfaces::ui_components::SettingsPanel;
use eframe::egui;

/// Helper to render a setting row with a label, input field, and tooltip hint
fn ui_setting_with_hint(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        // Larger text for labels to fill space better
        let _label_response = ui.label(egui::RichText::new(label).size(14.0));

        // Add a (?) hint icon
        ui.label(egui::RichText::new("(?)").weak().size(12.0))
            .on_hover_text(hint);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Substantially larger input field (200px)
            ui.add(
                egui::TextEdit::singleline(value)
                    .font(egui::TextStyle::Heading)
                    .desired_width(200.0),
            );
        });
    });
    // Add significant vertical spacing between rows
    ui.add_space(20.0);
}

/// Renders the Advanced Mode strategy settings
pub fn render_strategy_settings(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.add_space(20.0); // Space at top

    // --- Risk Management Group ---
    ui.group(|ui| {
        ui.label(
            egui::RichText::new(i18n.t("settings_group_risk"))
                .strong()
                .size(18.0),
        );
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
    ui.group(|ui| {
        ui.label(
            egui::RichText::new(i18n.t("settings_group_strategy"))
                .strong()
                .size(18.0),
        );
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
