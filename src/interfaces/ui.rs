use crate::application::agents::user_agent::UserAgent;
use chrono::{TimeZone, Utc};
use eframe::egui;
use egui_plot::{BoxElem, BoxSpread, Legend, Plot};
use rust_decimal::prelude::ToPrimitive;

impl eframe::App for UserAgent {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- 0. Theme Configuration (Run once or simple check) ---
        let mut visuals = egui::Visuals::dark();
        visuals.window_fill = egui::Color32::from_rgb(10, 15, 20); // Deep dark blue/black
        visuals.panel_fill = egui::Color32::from_rgb(10, 15, 20);
        
        // Improve Graph/Tooltip Interaction Visibility
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(240)); // Brighter text
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(200));
        
        // Ensure popups (tooltips) have a visible border and distinct background if needed
        visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(60));
        visuals.popup_shadow = egui::epaint::Shadow { offset: [2.0, 6.0].into(), blur: 8.0, spread: 0.0, color: egui::Color32::from_black_alpha(96) };
        
        ctx.set_visuals(visuals);

        // --- 1. Process System Events (Logs & Candles) ---
        self.update();
        ctx.request_repaint(); // Ensure continuous updates for logs/charts

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
                    
                    // Log Level Filter Buttons
                    ui.horizontal(|ui| {
                        ui.label("Filter:");
                        if ui.selectable_label(self.log_level_filter.is_none(), "All").clicked() {
                            self.log_level_filter = None;
                        }
                        if ui.selectable_label(
                            self.log_level_filter == Some("INFO".to_string()), 
                            "INFO"
                        ).clicked() {
                            self.log_level_filter = Some("INFO".to_string());
                        }
                        if ui.selectable_label(
                            self.log_level_filter == Some("WARN".to_string()), 
                            "WARN"
                        ).clicked() {
                            self.log_level_filter = Some("WARN".to_string());
                        }
                        if ui.selectable_label(
                            self.log_level_filter == Some("ERROR".to_string()), 
                            "ERROR"
                        ).clicked() {
                            self.log_level_filter = Some("ERROR".to_string());
                        }
                        if ui.selectable_label(
                            self.log_level_filter == Some("DEBUG".to_string()), 
                            "DEBUG"
                        ).clicked() {
                            self.log_level_filter = Some("DEBUG".to_string());
                        }
                    });
                    ui.separator();

                    // Chat History with filtering
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .max_height(ui.available_height() - 50.0) // Leave room for input
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for (sender, msg) in &self.chat_history {
                                // Apply log level filter
                                if let Some(ref filter_level) = self.log_level_filter {
                                    // Only filter System logs, not User/Agent messages
                                    if sender == "System" {
                                        if !msg.contains(filter_level.as_str()) {
                                            continue; // Skip this log entry
                                        }
                                    }
                                }
                                
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
            // Compact Header with Portfolio Summary
            ui.horizontal(|ui| {
                ui.heading("Portfolio Dashboard");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match self.portfolio.try_read() {
                        Ok(pf) => {
                            let mut cost_basis = rust_decimal::Decimal::ZERO;
                            for pos in pf.positions.values() {
                                cost_basis += pos.quantity * pos.average_price;
                            }
                            
                            ui.label(
                                egui::RichText::new(format!("Cost Basis: ${:.2}", cost_basis.to_f64().unwrap_or(0.0)))
                                    .color(egui::Color32::from_rgb(200, 200, 200))
                            );
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!("Cash: ${:.2}", pf.cash.to_f64().unwrap_or(0.0)))
                                    .strong()
                                    .color(egui::Color32::GREEN)
                                    .size(16.0)
                            );
                        }
                        Err(_) => {
                            ui.label("Syncing Portfolio...");
                        }
                    }
                });
            });
            
            ui.separator();

            match self.portfolio.try_read() {
                Ok(pf) => {
                    // Positions Table - Reduced Height
                    if !pf.positions.is_empty() {
                        ui.collapsing("Open Positions", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(150.0) // Reduced from 200.0
                                .show(ui, |ui| {
                                    egui::Grid::new("positions_grid")
                                        .striped(true)
                                        .min_col_width(100.0)
                                        .spacing([20.0, 5.0]) // tighter spacing
                                        .show(ui, |ui| {
                                            // Header
                                            ui.label(egui::RichText::new("SYMBOL").strong());
                                            ui.label(egui::RichText::new("QTY").strong());
                                            ui.label(egui::RichText::new("AVG").strong());
                                            ui.label(egui::RichText::new("TOTAL").strong());
                                            ui.end_row();

                                            // Rows
                                            for (symbol, pos) in &pf.positions {
                                                ui.label(
                                                    egui::RichText::new(symbol)
                                                        .strong()
                                                        .color(egui::Color32::GOLD),
                                                );
                                                ui.label(format!("{:.4}", pos.quantity.to_f64().unwrap_or(0.0)));
                                                ui.label(format!("${:.2}", pos.average_price.to_f64().unwrap_or(0.0)));
                                                let cost = pos.quantity * pos.average_price;
                                                ui.label(format!("${:.2}", cost.to_f64().unwrap_or(0.0)));
                                                ui.end_row();
                                            }
                                        });
                                });
                        });
                        ui.add_space(10.0);
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("Open Positions:");
                            ui.label(egui::RichText::new("None").italics().weak());
                        });
                        ui.add_space(10.0);
                    }
                }
                Err(_) => {
                    ui.spinner();
                }
            }

            ui.separator();

            // --- Tabs for Charts ---
            let mut symbols: Vec<_> = self.market_data.keys().cloned().collect();
            symbols.sort();

            if symbols.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("⏳ Waiting for market data... (Charts will appear when candles are received)");
                });
            } else {
                // Ensure we have a selected tab
                if self.selected_chart_tab.is_none()
                    || !symbols.contains(self.selected_chart_tab.as_ref().unwrap())
                {
                    self.selected_chart_tab = Some(symbols[0].clone());
                }

                // Tab buttons
                ui.horizontal(|ui| {
                    ui.label("Market:");
                    for symbol in &symbols {
                        let is_selected = self.selected_chart_tab.as_ref() == Some(symbol);
                        if ui.selectable_label(is_selected, symbol).clicked() {
                            self.selected_chart_tab = Some(symbol.clone());
                        }
                    }
                });

                ui.separator();

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

                                    // Calculate SMAs
                                    let fast_period = 20;
                                    let slow_period = 50;

                                    for (i, c) in candles.iter().enumerate() {
                                        // Use timestamp for X-axis
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
                                            BoxElem::new(
                                                t,
                                                BoxSpread::new(low, min_oc, mid, max_oc, high),
                                            )
                                            .fill(color)
                                            .stroke(egui::Stroke::new(1.0, color))
                                            .box_width(45.0), // ~75% of 60s
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
