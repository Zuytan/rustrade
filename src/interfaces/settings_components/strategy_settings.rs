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
            let desired_size = egui::vec2(120.0, 32.0);
            let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

            // Stable ID for the TextEdit focus tracking
            let id = ui.make_persistent_id(label);
            let has_focus = ui.memory(|mem| mem.focused() == Some(id));

            // Draw custom frame around text edit FIRST
            let stroke_color = if has_focus {
                DesignSystem::BORDER_FOCUS
            } else {
                DesignSystem::BORDER_SUBTLE
            };
            ui.painter()
                .rect_filled(rect, DesignSystem::ROUNDING_SMALL, DesignSystem::BG_INPUT);
            ui.painter().rect_stroke(
                rect,
                DesignSystem::ROUNDING_SMALL,
                egui::Stroke::new(1.0, stroke_color),
                egui::StrokeKind::Outside,
            );

            // Put the TextEdit inside the pre-allocated rect (with padding) SECOND
            let inner_rect = rect.shrink(4.0);
            ui.put(
                inner_rect,
                egui::TextEdit::singleline(value)
                    .id_source(label)
                    .font(egui::FontId::proportional(14.0))
                    .vertical_align(egui::Align::Center)
                    .frame(false), // Custom frame drawn above
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

            // Active Strategy Selection Dropdown
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(i18n.t("settings_strat_mode_label"))
                        .size(14.0)
                        .color(DesignSystem::TEXT_PRIMARY),
                );

                use crate::domain::market::strategy_config::StrategyMode;
                egui::ComboBox::from_id_salt("advanced_strategy_mode_select")
                    .selected_text(match panel.selected_strategy {
                        StrategyMode::RegimeAdaptive => i18n.t("strategy_mode_regime_adaptive"),
                        StrategyMode::SMC => i18n.t("strategy_mode_smc"),
                        StrategyMode::Ensemble => i18n.t("strategy_mode_ensemble"),
                        StrategyMode::ZScoreMR => i18n.t("strategy_mode_zscore_mr"),
                        StrategyMode::StatMomentum => i18n.t("strategy_mode_stat_momentum"),
                        StrategyMode::OrderFlow => i18n.t("strategy_mode_order_flow"),
                        StrategyMode::ML => i18n.t("strategy_mode_ml"),
                        StrategyMode::SnnSurrogate => i18n.t("strategy_mode_snn_surrogate"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::RegimeAdaptive,
                            i18n.t("strategy_mode_regime_adaptive"),
                        );
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::SMC,
                            i18n.t("strategy_mode_smc"),
                        );
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::Ensemble,
                            i18n.t("strategy_mode_ensemble"),
                        );
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::ZScoreMR,
                            i18n.t("strategy_mode_zscore_mr"),
                        );
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::StatMomentum,
                            i18n.t("strategy_mode_stat_momentum"),
                        );
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::OrderFlow,
                            i18n.t("strategy_mode_order_flow"),
                        );
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::ML,
                            i18n.t("strategy_mode_ml"),
                        );
                        ui.selectable_value(
                            &mut panel.selected_strategy,
                            StrategyMode::SnnSurrogate,
                            i18n.t("strategy_mode_snn_surrogate"),
                        );
                    });
            });
            ui.add_space(15.0);
            ui.separator();
            ui.add_space(15.0);

            // Timeframe Settings collapsing header
            ui.collapsing(i18n.t("settings_timeframe_config_title"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(i18n.t("settings_primary_timeframe"))
                            .size(14.0)
                            .color(DesignSystem::TEXT_PRIMARY),
                    );

                    let tf_options = ["1Min", "5Min", "15Min", "1Hour", "4Hour", "1Day"];
                    egui::ComboBox::from_id_salt("primary_timeframe_select")
                        .selected_text(&panel.primary_timeframe)
                        .show_ui(ui, |ui| {
                            for tf in tf_options {
                                ui.selectable_value(
                                    &mut panel.primary_timeframe,
                                    tf.to_string(),
                                    tf,
                                );
                            }
                        });
                });
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(i18n.t("settings_trend_timeframe"))
                            .size(14.0)
                            .color(DesignSystem::TEXT_PRIMARY),
                    );

                    let tf_options = ["1Min", "5Min", "15Min", "1Hour", "4Hour", "1Day"];
                    egui::ComboBox::from_id_salt("trend_timeframe_select")
                        .selected_text(&panel.trend_timeframe)
                        .show_ui(ui, |ui| {
                            for tf in tf_options {
                                ui.selectable_value(&mut panel.trend_timeframe, tf.to_string(), tf);
                            }
                        });
                });
                ui.add_space(10.0);

                ui_setting_with_hint(
                    ui,
                    i18n.t("settings_trend_sma_period"),
                    &mut panel.trend_sma_period,
                    i18n.t("settings_trend_sma_period_hint"),
                );

                ui.separator();
                ui.add_space(10.0);
            });

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

    ui.add_space(40.0); // More space between groups
}
