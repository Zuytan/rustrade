use crate::application::agents::user_agent::UserAgent;
use chrono::Utc;
use eframe::egui;
use egui_plot::{BoxElem, BoxSpread, Legend, Plot};
use rust_decimal::prelude::ToPrimitive;

impl eframe::App for UserAgent {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- 0. Theme Configuration (Run once or simple check) ---
        let mut visuals = egui::Visuals::dark();
        visuals.window_fill = egui::Color32::from_rgb(10, 15, 20); // Deep dark blue/black
        visuals.panel_fill = egui::Color32::from_rgb(10, 15, 20);
        ctx.set_visuals(visuals);

        // --- 1. Process System Events (Logs & Candles) ---
        self.update();

        // --- 2. Top Status Bar ---
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🦀 Rustrade Agent");
                ui.separator();
                ui.label(format!("Time (UTC): {}", Utc::now().format("%H:%M:%S")));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("● ONLINE")
                            .color(egui::Color32::GREEN)
                            .small(),
                    );
                });
            });
        });

        // --- 3. Left Sidebar: Chat & Logs ---
        egui::SidePanel::left("chat_panel")
            .default_width(350.0)
            .min_width(250.0)
            .max_width(600.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.heading("System Logs & Chat");
                    ui.separator();

                    // Chat History
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .max_height(ui.available_height() - 50.0) // Leave room for input
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for (sender, msg) in &self.chat_history {
                                ui.horizontal_wrapped(|ui| {
                                    let (label_text, color) = match sender.as_str() {
                                        "User" => {
                                            ("User >", egui::Color32::from_rgb(100, 200, 255))
                                        } // Cyan
                                        "Agent" => {
                                            ("Agent <", egui::Color32::from_rgb(255, 200, 100))
                                        } // Gold
                                        "System" => {
                                            if msg.contains("ERROR") {
                                                ("System !", egui::Color32::from_rgb(255, 80, 80))
                                            // Red
                                            } else if msg.contains("WARN") {
                                                ("System ?", egui::Color32::from_rgb(255, 255, 100))
                                            // Yellow
                                            } else {
                                                ("System :", egui::Color32::from_rgb(180, 180, 180))
                                                // Gray
                                            }
                                        }
                                        _ => (sender.as_str(), egui::Color32::WHITE),
                                    };

                                    ui.label(egui::RichText::new(label_text).strong().color(color));
                                    ui.label(
                                        egui::RichText::new(msg)
                                            .color(egui::Color32::from_gray(220)),
                                    );
                                });
                            }
                        });

                    ui.separator();

                    // Input Area
                    ui.horizontal(|ui| {
                        ui.label("Cmd >");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.input_text)
                                .desired_width(f32::INFINITY),
                        );

                        if self.is_focused {
                            response.request_focus();
                            self.is_focused = false;
                        }

                        if ui.button("Send").clicked()
                            || (response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            if !self.input_text.trim().is_empty() {
                                if let Some(reply) = self.process_input() {
                                    self.chat_history.push(("Agent".to_string(), reply));
                                }
                                self.input_text.clear();
                            }
                            self.is_focused = true; // Refocus after send
                        }
                    });
                });
            });

        // --- 4. Central Panel: Dashboard ---
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Portfolio Dashboard");
            ui.add_space(10.0);

            if let Ok(pf) = self.portfolio.try_read() {
                // KPI Cards
                ui.horizontal(|ui| {
                    metric_card(ui, "Cash Available", pf.cash.to_f64().unwrap_or(0.0), "$");

                    // Approximate cost basis
                    let mut cost_basis = rust_decimal::Decimal::ZERO;
                    for pos in pf.positions.values() {
                        cost_basis += pos.quantity * pos.average_price;
                    }
                    metric_card(ui, "Cost Basis", cost_basis.to_f64().unwrap_or(0.0), "$");
                });

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);

                // Positions Table
                ui.heading("Open Positions");
                ui.add_space(5.0);

                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        egui::Grid::new("positions_grid")
                            .striped(true)
                            .min_col_width(100.0)
                            .spacing([20.0, 10.0])
                            .show(ui, |ui| {
                                // Header
                                ui.label(egui::RichText::new("SYMBOL").strong().heading());
                                ui.label(egui::RichText::new("QTY").strong().heading());
                                ui.label(egui::RichText::new("AVG PRICE").strong().heading());
                                ui.label(egui::RichText::new("COST BASIS").strong().heading());
                                ui.end_row();

                                // Rows
                                for (symbol, pos) in &pf.positions {
                                    ui.label(
                                        egui::RichText::new(symbol)
                                            .strong()
                                            .size(16.0)
                                            .color(egui::Color32::GOLD),
                                    );
                                    ui.label(format!(
                                        "{:.4}",
                                        pos.quantity.to_f64().unwrap_or(0.0)
                                    ));
                                    ui.label(format!(
                                        "${:.2}",
                                        pos.average_price.to_f64().unwrap_or(0.0)
                                    ));

                                    let cost = pos.quantity * pos.average_price;
                                    ui.label(format!("${:.2}", cost.to_f64().unwrap_or(0.0)));
                                    ui.end_row();
                                }

                                if pf.positions.is_empty() {
                                    ui.label("No active positions.");
                                    ui.end_row();
                                }
                            });
                    });
            } else {
                ui.colored_label(egui::Color32::RED, "Portfolio Locked (Data updating...)");
            }

            ui.add_space(20.0);
            ui.separator();
            ui.heading("Live Markets (M1 Candles)");
            ui.add_space(10.0);

            // --- Tabs for Charts ---
            let mut symbols: Vec<_> = self.market_data.keys().cloned().collect();
            symbols.sort();

            if symbols.is_empty() {
                ui.label(
                    "⏳ Waiting for market data... (Charts will appear when candles are received)",
                );
            } else {
                // Ensure we have a selected tab
                if self.selected_chart_tab.is_none()
                    || !symbols.contains(self.selected_chart_tab.as_ref().unwrap())
                {
                    self.selected_chart_tab = Some(symbols[0].clone());
                }

                // Tab buttons
                ui.horizontal(|ui| {
                    for symbol in &symbols {
                        let is_selected = self.selected_chart_tab.as_ref() == Some(symbol);
                        if ui.selectable_label(is_selected, symbol).clicked() {
                            self.selected_chart_tab = Some(symbol.clone());
                        }
                    }
                });

                ui.separator();
                ui.add_space(10.0);

                // Chart for selected tab
                if let Some(selected_symbol) = &self.selected_chart_tab {
                    if let Some(candles) = self.market_data.get(selected_symbol) {
                        if candles.is_empty() {
                            ui.label(format!("No candles yet for {}", selected_symbol));
                        } else {
                            // Strategy Info Panel
                            if let Some(strat_info) = self.strategy_info.get(selected_symbol) {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Strategy:").strong());
                                    ui.label(&strat_info.mode);
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "SMA: {}/{}",
                                            strat_info.fast_sma as i32, strat_info.slow_sma as i32
                                        ))
                                        .small(),
                                    );
                                    if let Some(signal) = &strat_info.last_signal {
                                        ui.separator();
                                        let signal_color = if signal.contains("Buy")
                                            || signal.contains("Golden")
                                        {
                                            egui::Color32::GREEN
                                        } else {
                                            egui::Color32::RED
                                        };
                                        ui.label(
                                            egui::RichText::new(signal).color(signal_color).small(),
                                        );
                                    }
                                });
                            }

                            ui.label(format!(
                                "📊 {} ({} candles)",
                                selected_symbol,
                                candles.len()
                            ));
                            ui.add_space(5.0);

                            let height = ui.available_height() - 20.0;
                            Plot::new(format!("chart_{}", selected_symbol))
                                .height(height.max(400.0))
                                .show_grid([true, true])
                                .legend(Legend::default())
                                .show(ui, |plot_ui| {
                                    let mut box_elems = Vec::new();
                                    let mut fast_sma_points = Vec::new();
                                    let mut slow_sma_points = Vec::new();

                                    // Calculate SMAs
                                    let fast_period = 20;
                                    let slow_period = 50;

                                    for (i, c) in candles.iter().enumerate() {
                                        let t = i as f64;
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
                                            BoxElem::new(
                                                t,
                                                BoxSpread::new(low, min_oc, mid, max_oc, high),
                                            )
                                            .fill(color)
                                            .stroke(egui::Stroke::new(1.0, color))
                                            .box_width(0.8),
                                        );

                                        // Calculate SMAs
                                        if i >= fast_period - 1 {
                                            let fast_sum: f64 = candles[i - (fast_period - 1)..=i]
                                                .iter()
                                                .map(|c| c.close.to_f64().unwrap_or(0.0))
                                                .sum();
                                            fast_sma_points
                                                .push([t, fast_sum / fast_period as f64]);
                                        }

                                        if i >= slow_period - 1 {
                                            let slow_sum: f64 = candles[i - (slow_period - 1)..=i]
                                                .iter()
                                                .map(|c| c.close.to_f64().unwrap_or(0.0))
                                                .sum();
                                            slow_sma_points
                                                .push([t, slow_sum / slow_period as f64]);
                                        }
                                    }

                                    // Draw candles
                                    plot_ui.box_plot(
                                        egui_plot::BoxPlot::new(box_elems).name(selected_symbol),
                                    );

                                    // Draw SMA lines
                                    if !fast_sma_points.is_empty() {
                                        plot_ui.line(
                                            egui_plot::Line::new(fast_sma_points)
                                                .color(egui::Color32::from_rgb(100, 200, 255))
                                                .name("SMA 20"),
                                        );
                                    }

                                    if !slow_sma_points.is_empty() {
                                        plot_ui.line(
                                            egui_plot::Line::new(slow_sma_points)
                                                .color(egui::Color32::from_rgb(255, 165, 0))
                                                .name("SMA 50"),
                                        );
                                    }
                                });
                        }
                    }
                }
            }
        });

        // Force frequent repaints to ensure responsive logs
        ctx.request_repaint();
    }
}

// Helper for Stat Cards
fn metric_card(ui: &mut egui::Ui, label: &str, value: f64, prefix: &str) {
    ui.group(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .small()
                    .color(egui::Color32::GRAY),
            );
            ui.label(
                egui::RichText::new(format!("{}{:.2}", prefix, value))
                    .heading()
                    .strong()
                    .color(egui::Color32::WHITE),
            );
        });
    });
}
