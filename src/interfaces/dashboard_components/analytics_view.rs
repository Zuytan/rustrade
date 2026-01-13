use crate::application::agents::user_agent::UserAgent;
use crate::interfaces::dashboard_components::metrics_card::render_mini_metric;
use eframe::egui;
use rust_decimal::prelude::ToPrimitive;

/// Renders the Analytics View (Monte Carlo & Correlation)
pub fn render_analytics_view(ui: &mut egui::Ui, agent: &mut UserAgent) {
    ui.vertical(|ui| {
        ui.add_space(10.0);

        // Header
        ui.heading(
            egui::RichText::new(format!("🔬 {}", agent.i18n.t("analytics_title")))
                .size(24.0)
                .strong()
                .color(egui::Color32::WHITE),
        );
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(20.0);

        egui::ScrollArea::vertical()
            .id_salt("analytics_scroll")
            .show(ui, |ui| {
                // --- SECTION 1: MONTE CARLO ---
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(agent.i18n.t("monte_carlo_title")).size(18.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(egui::RichText::new(agent.i18n.t("run_simulation")).strong()).clicked() {
                                    // Trigger simulation
                                    let (avg_win, avg_loss) = agent.calculate_trade_statistics();

                                    let config = crate::domain::performance::monte_carlo::MonteCarloConfig {
                                        iterations: 10000,
                                        steps: 100,
                                        initial_equity: agent.calculate_total_value(),
                                        win_rate: agent.calculate_win_rate() / 100.0,
                                        avg_win_pct: avg_win,
                                        avg_loss_pct: avg_loss,
                                    };
                                    agent.monte_carlo_result = Some(crate::domain::performance::monte_carlo::MonteCarloEngine::simulate(&config));
                                }
                            });
                        });
                        ui.label(egui::RichText::new(agent.i18n.t("monte_carlo_description")).size(11.0).color(egui::Color32::from_gray(140)));
                        ui.add_space(15.0);

                        if let Some(res) = &agent.monte_carlo_result {
                            ui.columns(4, |cols| {
                                render_mini_metric(&mut cols[0], agent.i18n.t("prob_profit").to_string(), &format!("{:.1}%", res.probability_of_profit * 100.0), egui::Color32::from_rgb(0, 230, 118));
                                render_mini_metric(&mut cols[1], agent.i18n.t("expected_dd").to_string(), &format!("{:.1}%", res.max_drawdown_mean * 100.0), egui::Color32::from_rgb(255, 23, 68));
                                render_mini_metric(&mut cols[2], agent.i18n.t("final_equity").to_string(), &format!("${:.0}", res.final_equity_median.to_f64().unwrap_or(0.0)), egui::Color32::WHITE);
                                render_mini_metric(&mut cols[3], "95% Range".to_string(), &format!("${:.0} - ${:.0}", res.percentile_5.to_f64().unwrap_or(0.0), res.percentile_95.to_f64().unwrap_or(0.0)), egui::Color32::from_gray(160));
                            });
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label(egui::RichText::new("No simulation data. Click 'Run' to project equity paths.").italics().color(egui::Color32::from_gray(100)));
                            });
                        }
                    });
                });

                ui.add_space(30.0);

                // --- SECTION 2: CORRELATION MATRIX ---
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(agent.i18n.t("correlation_title")).size(18.0).strong());
                        ui.label(egui::RichText::new(agent.i18n.t("correlation_description")).size(11.0).color(egui::Color32::from_gray(140)));
                        ui.add_space(15.0);

                        render_correlation_heatmap(ui, agent);
                    });
                });
            });
    });
}

fn render_correlation_heatmap(ui: &mut egui::Ui, agent: &UserAgent) {
    let mut symbols: Vec<_> = agent.market_data.keys().cloned().collect();
    symbols.sort();

    if symbols.is_empty() {
        ui.label(
            egui::RichText::new("Waiting for market data symbols...")
                .italics()
                .color(egui::Color32::from_gray(100)),
        );
        return;
    }

    let cell_size = 60.0;
    let label_width = 80.0;

    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.vertical(|ui| {
            // Header Row (Symbols)
            ui.horizontal(|ui| {
                ui.add_space(label_width);
                for sym in &symbols {
                    ui.allocate_ui(egui::vec2(cell_size, 20.0), |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label(egui::RichText::new(sym).size(10.0).strong());
                        });
                    });
                }
            });

            // Data Rows
            for s1 in &symbols {
                ui.horizontal(|ui| {
                    ui.allocate_ui(egui::vec2(label_width, cell_size), |ui| {
                        ui.label(egui::RichText::new(s1).size(10.0).strong());
                    });

                    for s2 in &symbols {
                        // Get correlation from agent state
                        let corr = agent
                            .correlation_matrix
                            .get(&(s1.clone(), s2.clone()))
                            .cloned()
                            .unwrap_or(0.0);

                        // Color mapping: Red (-1) -> Black (0) -> Green (1)
                        let color = if corr > 0.0 {
                            egui::Color32::from_rgb(0, (230.0 * corr) as u8, 118)
                                .linear_multiply(0.5 + (0.5 * corr) as f32)
                        } else {
                            egui::Color32::from_rgb((255.0 * corr.abs()) as u8, 23, 68)
                                .linear_multiply(0.5 + (0.5 * corr.abs()) as f32)
                        };

                        let (rect, _response) = ui.allocate_exact_size(
                            egui::vec2(cell_size, cell_size),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(rect.shrink(1.0), 2.0, color);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{:.2}", corr),
                            egui::FontId::proportional(10.0),
                            egui::Color32::WHITE,
                        );
                    }
                });
            }
        });
    });
}
