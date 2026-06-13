//! Risk settings component (Simple Mode)

use crate::domain::risk::optimal_parameters::{AssetType, OptimalParameters};
use crate::domain::risk::risk_appetite::{RiskAppetite, RiskProfile};
use crate::infrastructure::i18n::I18nService;
use crate::infrastructure::optimal_parameters_persistence::OptimalParametersPersistence;
use crate::interfaces::components::card::Card;
use crate::interfaces::design_system::DesignSystem;
use crate::interfaces::ui_components::SettingsPanel;
use eframe::egui;
use rust_decimal::Decimal;

/// Renders the Simple Mode risk settings with score slider
pub fn render_risk_settings(ui: &mut egui::Ui, panel: &mut SettingsPanel, i18n: &I18nService) {
    ui.add_space(30.0); // Space at top

    Card::new()
        .title(i18n.t("settings_risk_score_label"))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(i18n.t("settings_risk_score_hint"))
                    .color(DesignSystem::TEXT_SECONDARY)
                    .size(14.0),
            );
            ui.add_space(40.0); // More space before slider

            let mut score_f32 = panel.risk_score as f32;

            // Custom styling for slider (wider handle, accent color)
            ui.spacing_mut().slider_width = 300.0;
            let slider = egui::Slider::new(&mut score_f32, 1.0..=10.0)
                .step_by(1.0)
                .show_value(true);

            ui.add(slider);

            if score_f32 as u8 != panel.risk_score {
                panel.risk_score = score_f32 as u8;
                panel.update_from_score(panel.risk_score);
            }

            ui.add_space(40.0); // More space after slider

            // Show derived profile badge
            if let Ok(appetite) = RiskAppetite::new(panel.risk_score) {
                let (profile_key, color) = match appetite.profile() {
                    RiskProfile::Conservative => ("profile_conservative", DesignSystem::SUCCESS), // Green
                    RiskProfile::Balanced => ("profile_balanced", DesignSystem::WARNING), // Yellow
                    RiskProfile::Aggressive => ("profile_aggressive", DesignSystem::DANGER), // Red
                };
                let profile_text = i18n.t(profile_key);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(i18n.t("settings_risk_profile_label"))
                            .size(18.0)
                            .color(DesignSystem::TEXT_PRIMARY),
                    ); // Larger
                    ui.label(
                        egui::RichText::new(profile_text)
                            .strong()
                            .size(18.0)
                            .color(color),
                    ); // Larger
                });

                ui.add_space(20.0);

                // Show selected strategy as a dropdown
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(i18n.t("settings_risk_strategy_label"))
                            .size(18.0)
                            .color(DesignSystem::TEXT_PRIMARY),
                    );

                    use crate::domain::market::strategy_config::StrategyMode;
                    egui::ComboBox::from_id_salt("simple_strategy_mode_select")
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

                ui.add_space(30.0); // More space

                // Make derived stats prominent
                egui::Frame::NONE
                    .fill(DesignSystem::BG_INPUT)
                    .corner_radius(DesignSystem::ROUNDING_MEDIUM)
                    .inner_margin(DesignSystem::SPACING_MEDIUM)
                    .show(ui, |ui| {
                        let stats = [
                            (
                                i18n.t("settings_stat_risk_per_trade"),
                                format!(
                                    "{:.1}%",
                                    appetite.calculate_risk_per_trade_percent()
                                        * Decimal::from(100)
                                ),
                            ),
                            (
                                i18n.t("settings_stat_max_drawdown"),
                                format!(
                                    "{:.1}%",
                                    panel
                                        .max_drawdown_pct
                                        .parse::<Decimal>()
                                        .unwrap_or(Decimal::ZERO)
                                        * Decimal::from(100)
                                ),
                            ),
                            (
                                i18n.t("settings_stat_target_profit"),
                                format!(
                                    "{:.1}x ATR",
                                    appetite.calculate_profit_target_multiplier()
                                ),
                            ),
                        ];

                        for (label, value) in stats {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(label)
                                        .color(DesignSystem::TEXT_SECONDARY)
                                        .size(14.0),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(value)
                                                .color(DesignSystem::TEXT_PRIMARY)
                                                .strong()
                                                .size(14.0),
                                        );
                                    },
                                );
                            });
                            ui.add_space(4.0);
                        }
                    });

                // Apply Optimal Settings button
                ui.add_space(20.0);
                render_optimal_settings_button(ui, panel, appetite.profile(), i18n);
            }
        });

    ui.add_space(30.0); // Space at bottom
}

/// Renders the "Apply Optimal Settings" button and metadata if available.
fn render_optimal_settings_button(
    ui: &mut egui::Ui,
    panel: &mut SettingsPanel,
    profile: RiskProfile,
    i18n: &I18nService,
) {
    // Prefer optimal params for exact risk score (from optimize --risk-score N), else profile
    let optimal =
        load_optimal_for_risk_score(panel.risk_score).or_else(|| load_optimal_for_profile(profile));

    match optimal {
        Some(params) => {
            // Show Apply button
            let button = egui::Button::new(
                egui::RichText::new(i18n.t("settings_risk_apply_optimal"))
                    .size(14.0)
                    .color(DesignSystem::TEXT_PRIMARY),
            )
            .fill(DesignSystem::ACCENT_PRIMARY);

            if ui.add(button).clicked() {
                apply_optimal_to_panel(panel, &params);
            }

            // Show metadata
            ui.add_space(8.0);

            let date_str = params.optimization_date.format("%Y-%m-%d").to_string();
            let optimized_on_text = i18n.tf(
                "settings_risk_optimized_on",
                &[("date", &date_str), ("symbol", &params.symbol_used)],
            );
            ui.label(
                egui::RichText::new(optimized_on_text)
                    .color(DesignSystem::TEXT_SECONDARY)
                    .size(12.0),
            );

            let sharpe_str = format!("{:.2}", params.sharpe_ratio);
            let return_str = format!("{:.1}", params.total_return);
            let win_rate_str = format!("{:.0}", params.win_rate);
            let metrics_text = i18n.tf(
                "settings_risk_optimized_metrics",
                &[
                    ("sharpe", &sharpe_str),
                    ("return", &return_str),
                    ("win_rate", &win_rate_str),
                ],
            );
            ui.label(
                egui::RichText::new(metrics_text)
                    .color(DesignSystem::TEXT_SECONDARY)
                    .size(12.0),
            );
        }
        None => {
            // Show disabled state or hint
            ui.label(
                egui::RichText::new(i18n.t("settings_risk_optimize_hint"))
                    .color(DesignSystem::TEXT_SECONDARY)
                    .size(12.0)
                    .italics(),
            );
        }
    }
}

/// Loads optimal parameters for the given risk score (1-9). Prefers exact score, then profile.
fn load_optimal_for_risk_score(score: u8) -> Option<OptimalParameters> {
    OptimalParametersPersistence::new()
        .ok()?
        .get_for_risk_score(score, AssetType::Stock)
        .ok()
        .flatten()
}

/// Loads optimal parameters for a given risk profile from persistence.
fn load_optimal_for_profile(profile: RiskProfile) -> Option<OptimalParameters> {
    OptimalParametersPersistence::new()
        .ok()?
        .get_for_profile(profile)
        .ok()
        .flatten()
}

/// Applies optimal parameters to the settings panel.
fn apply_optimal_to_panel(panel: &mut SettingsPanel, params: &OptimalParameters) {
    panel.fast_sma_period = params.fast_sma_period.to_string();
    panel.slow_sma_period = params.slow_sma_period.to_string();
    panel.rsi_threshold = params.rsi_threshold.to_string();
    // Note: Some fields (trailing_stop, trend_divergence, cooldown) are not in SettingsPanel
    // They are applied directly via AnalystConfig when saving
}
