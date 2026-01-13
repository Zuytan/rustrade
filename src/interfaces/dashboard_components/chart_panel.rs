use crate::application::agents::user_agent::UserAgent;
use chrono::{TimeZone, Utc};
use eframe::egui;
use egui_plot::{BoxElem, BoxSpread, Legend, Plot};
use rust_decimal::prelude::ToPrimitive;

/// Helper function to render the chart panel (Moved from ui.rs)
pub fn render_chart_panel(agent: &mut UserAgent, ui: &mut egui::Ui) {
    // --- Tabs for Charts ---
    let mut symbols: Vec<_> = agent.market_data.keys().cloned().collect();
    symbols.sort();

    if symbols.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(agent.i18n.t("waiting_market_data"));
        });
    } else {
        // Ensure we have a selected tab
        if agent.selected_chart_tab.is_none()
            || !symbols.contains(agent.selected_chart_tab.as_ref().unwrap())
        {
            agent.selected_chart_tab = Some(symbols[0].clone());
        }

        // --- Selection moved to right panel (no tabs here anymore) ---
        ui.horizontal(|ui| {
            if let Some(selected_symbol) = &agent.selected_chart_tab {
                ui.label(
                    egui::RichText::new(
                        agent
                            .i18n
                            .tf("live_market_format", &[("symbol", selected_symbol)]),
                    )
                    .strong()
                    .size(16.0)
                    .color(egui::Color32::WHITE),
                );
            }
        });

        ui.add_space(8.0);

        // Chart for selected tab
        if let Some(selected_symbol) = &agent.selected_chart_tab
            && let Some(candles) = agent.market_data.get(selected_symbol)
        {
            if candles.is_empty() {
                ui.label(agent.i18n.tf("no_candles", &[("symbol", selected_symbol)]));
            } else {
                // Info Panel
                if let Some(strat_info) = agent.strategy_info.get(selected_symbol) {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(22, 27, 34))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .corner_radius(6)
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(agent.i18n.t("strategy_label"))
                                        .strong()
                                        .color(egui::Color32::from_gray(160)),
                                );

                                let strategy_display =
                                    if strat_info.mode.to_lowercase() == "dynamicregime" {
                                        if let Some(signal) = &strat_info.last_signal {
                                            if signal.contains("Dynamic (Trend)") {
                                                agent.i18n.t("dynamic_trend").to_string()
                                            } else if signal.contains("Dynamic (Choppy)") {
                                                agent.i18n.t("dynamic_choppy").to_string()
                                            } else {
                                                agent.i18n.t("dynamic").to_string()
                                            }
                                        } else {
                                            agent.i18n.t("dynamic").to_string()
                                        }
                                    } else {
                                        strat_info.mode.clone()
                                    };

                                ui.label(
                                    egui::RichText::new(&strategy_display)
                                        .color(egui::Color32::from_rgb(88, 166, 255)),
                                );
                                ui.separator();
                                ui.label(
                                    egui::RichText::new(agent.i18n.tf(
                                        "sma_label",
                                        &[
                                            ("fast", &strat_info.fast_sma.to_string()),
                                            ("slow", &strat_info.slow_sma.to_string()),
                                        ],
                                    ))
                                    .color(egui::Color32::from_gray(160))
                                    .size(11.0),
                                );
                            });
                        });
                    ui.add_space(6.0);
                }

                // The Plot
                let height = ui.available_height() - 20.0;
                Plot::new(format!("chart_{}", selected_symbol))
                    .height(height.max(300.0))
                    .show_grid([true, true])
                    .legend(Legend::default())
                    .x_axis_formatter(|mark, _range| {
                        let dt = Utc.timestamp_opt(mark.value as i64, 0).unwrap();
                        dt.format("%H:%M:%S").to_string()
                    })
                    .show(ui, |plot_ui| {
                        let mut box_elems = Vec::new();
                        let mut fast_sma_points = Vec::new();
                        let mut slow_sma_points = Vec::new();
                        let fast_period = 20;
                        let slow_period = 50;

                        for (i, c) in candles.iter().enumerate() {
                            let t = c.timestamp as f64;
                            let open = c.open.to_f64().unwrap_or(0.0);
                            let close = c.close.to_f64().unwrap_or(0.0);
                            let high = c.high.to_f64().unwrap_or(0.0);
                            let low = c.low.to_f64().unwrap_or(0.0);
                            let color = if close >= open {
                                egui::Color32::GREEN
                            } else {
                                egui::Color32::RED
                            };
                            let min_oc = open.min(close);
                            let max_oc = open.max(close);
                            let mid = (open + close) / 2.0;

                            box_elems.push(
                                BoxElem::new(t, BoxSpread::new(low, min_oc, mid, max_oc, high))
                                    .fill(color)
                                    .stroke(egui::Stroke::new(1.0, color))
                                    .box_width(45.0),
                            );

                            if i >= fast_period - 1 {
                                let fast_sum: f64 = candles[i - (fast_period - 1)..=i]
                                    .iter()
                                    .map(|c| c.close.to_f64().unwrap_or(0.0))
                                    .sum();
                                fast_sma_points.push([t, fast_sum / fast_period as f64]);
                            }
                            if i >= slow_period - 1 {
                                let slow_sum: f64 = candles[i - (slow_period - 1)..=i]
                                    .iter()
                                    .map(|c| c.close.to_f64().unwrap_or(0.0))
                                    .sum();
                                slow_sma_points.push([t, slow_sum / slow_period as f64]);
                            }
                        }

                        plot_ui
                            .box_plot(egui_plot::BoxPlot::new(selected_symbol.clone(), box_elems));

                        if !fast_sma_points.is_empty() {
                            plot_ui.line(
                                egui_plot::Line::new(agent.i18n.t("sma_20_label"), fast_sma_points)
                                    .color(egui::Color32::from_rgb(100, 200, 255)),
                            );
                        }
                        if !slow_sma_points.is_empty() {
                            plot_ui.line(
                                egui_plot::Line::new(agent.i18n.t("sma_50_label"), slow_sma_points)
                                    .color(egui::Color32::from_rgb(255, 165, 0)),
                            );
                        }
                    });
            }
        }
    }
}
