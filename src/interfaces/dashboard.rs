use crate::application::agents::user_agent::{UserAgent, ActivityEventType, EventSeverity, ActivityEvent};
use eframe::egui;
use egui_plot::{BoxElem, BoxSpread, Legend, Plot};
use chrono::{TimeZone, Utc};
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Renders the main Dashboard content (Concept Art Layout)
pub fn render_dashboard(ui: &mut egui::Ui, agent: &mut UserAgent) {
    // --- Data Prep ---
    let total_value = agent.calculate_total_value();
    let (cash, position_count, unrealized_pnl, unrealized_pct, market_value) = match agent.portfolio.try_read() {
        Ok(pf) => {
            let mut cost_basis = rust_decimal::Decimal::ZERO;
            let mut mv = rust_decimal::Decimal::ZERO;
            for (symbol, pos) in pf.positions.iter() {
                let position_cost = pos.quantity * pos.average_price;
                cost_basis += position_cost;
                if let Some(info) = agent.strategy_info.get(symbol) {
                    mv += pos.quantity * info.current_price;
                } else {
                    mv += position_cost;
                }
            }
            let pnl = mv - cost_basis;
            let pnl_pct = if cost_basis > rust_decimal::Decimal::ZERO {
                (pnl / cost_basis * rust_decimal::Decimal::from(100)).to_f64().unwrap_or(0.0)
            } else { 0.0 };
            (pf.cash, pf.positions.len(), pnl, pnl_pct, mv)
        }
        Err(_) => (rust_decimal::Decimal::ZERO, 0, rust_decimal::Decimal::ZERO, 0.0, rust_decimal::Decimal::ZERO),
    };
    
    let win_rate = agent.calculate_win_rate();

    // ---------------------------------------------------------
    // 1. TOP HEADER (Total Value + System Status)
    // ---------------------------------------------------------
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        // Left: Total Value
        ui.vertical(|ui| {
             ui.horizontal(|ui| {
                 ui.heading(
                    egui::RichText::new(format!("Total Value: ${:.2}", total_value.to_f64().unwrap_or(0.0)))
                        .size(28.0)
                        .strong()
                        .color(egui::Color32::WHITE)
                );
                
                ui.add_space(10.0);
                
                // Small P&L Pill
                let pnl_color = if unrealized_pnl >= rust_decimal::Decimal::ZERO {
                    egui::Color32::from_rgb(0, 230, 118) // Neon Green
                } else {
                    egui::Color32::from_rgb(255, 23, 68) // Neon Red
                };
                let pnl_sign = if unrealized_pnl >= rust_decimal::Decimal::ZERO { "+" } else { "" };
                
                egui::Frame::none()
                    .fill(pnl_color.linear_multiply(0.15))
                    .rounding(12.0)
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                         ui.label(
                             egui::RichText::new(format!("{}{:.2} ({:.2}%)", pnl_sign, unrealized_pnl.to_f64().unwrap_or(0.0).abs(), unrealized_pct))
                                 .size(12.0)
                                 .strong()
                                 .color(pnl_color)
                         );
                    });
             });
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
             // System Status
             ui.group(|ui| {
                ui.set_style(ui.style().clone()); // Reset style
                 ui.horizontal(|ui| {
                     ui.label(egui::RichText::new("●").size(10.0).color(egui::Color32::GREEN));
                     ui.label(
                         egui::RichText::new("System Status: Active - HFT Engine Running")
                             .size(12.0)
                             .color(egui::Color32::from_gray(160))
                     );
                     ui.add_space(10.0);
                     ui.label(
                         egui::RichText::new("Latency: 12ms")
                            .size(12.0)
                            .color(egui::Color32::from_gray(100))
                     );
                 });
             });
        });
    });
    
    ui.add_space(20.0);

    // ---------------------------------------------------------
    // 2. METRICS CARDS (4 Columns)
    // ---------------------------------------------------------
    ui.columns(4, |columns| {
        // Card 1: DAILY P&L (Active Blue Border Effect)
        let pnl_val = unrealized_pnl.to_f64().unwrap_or(0.0);
        let pnl_color = if pnl_val >= 0.0 { egui::Color32::from_rgb(0, 230, 118) } else { egui::Color32::from_rgb(255, 23, 68) };
        
        columns[0].push_id("card_daily_pnl", |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(22, 27, 34)) 
                .rounding(10.0)
                .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(41, 121, 255))) // Blue Active Stroke
                .shadow(egui::epaint::Shadow {
                    offset: [0.0, 4.0].into(),
                    blur: 15.0,
                    spread: 0.0,
                    color: egui::Color32::from_rgba_premultiplied(41, 121, 255, 40), // Blue Glow
                })
                .inner_margin(16.0)
                .show(ui, |ui| {
                     ui.set_min_height(100.0);
                     ui.label(egui::RichText::new("DAILY P&L").size(12.0).color(egui::Color32::from_gray(140)).strong());
                     ui.add_space(8.0);
                     
                     ui.horizontal(|ui| {
                         let sign = if pnl_val >= 0.0 { "+" } else { "" };
                         ui.label(egui::RichText::new(format!("{}{:.2}", sign, pnl_val.abs())).size(28.0).strong().color(pnl_color));
                         ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                             ui.label(egui::RichText::new("↗").size(18.0).color(pnl_color));
                         });
                     });
                     ui.add_space(4.0);
                     ui.label(egui::RichText::new("Last 24h").size(11.0).color(egui::Color32::from_gray(100)));
                });
        });

        // Card 2: WIN RATE (Circle)
        columns[1].push_id("card_win_rate", |ui| {
             render_start_card(ui, "WIN RATE", |ui| {
                 ui.horizontal(|ui| {
                     // Text
                     ui.vertical(|ui| {
                         ui.label(egui::RichText::new(format!("{:.1}%", win_rate)).size(28.0).strong().color(egui::Color32::from_rgb(56, 139, 253)));
                         ui.label(egui::RichText::new(format!("{}/{} Trades", agent.winning_trades, agent.total_trades)).size(11.0).color(egui::Color32::from_gray(120)));
                     });
                     
                     // Donut Chart (Simulated)
                     ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                         let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                         ui.painter().circle_stroke(rect.center(), 18.0, egui::Stroke::new(4.0, egui::Color32::from_gray(40))); // Track
                         
                         // Arc (Arc simulation)
                         // For full implementation we need complex path, simple circle for now
                         ui.painter().circle_stroke(rect.center(), 18.0, egui::Stroke::new(4.0, egui::Color32::from_rgb(56, 139, 253))); // Progress
                     });
                 });
             });
        });

        // Card 3: OPEN POSITIONS (Icon)
        columns[2].push_id("card_open_pos", |ui| {
             render_start_card(ui, "OPEN POSITIONS", |ui| {
                 ui.horizontal(|ui| {
                     ui.vertical(|ui| {
                         ui.label(egui::RichText::new(format!("{}", position_count)).size(28.0).strong().color(egui::Color32::WHITE));
                         ui.label(egui::RichText::new(format!("Total Volume: ${:.0}", market_value)).size(11.0).color(egui::Color32::from_gray(120)));
                     });
                     
                     ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                         ui.label(egui::RichText::new("🪙").size(24.0).color(egui::Color32::from_gray(100)));
                     });
                 });
             });
        });

        // Card 4: RISK SCORE (Shield)
        columns[3].push_id("card_risk", |ui| {
             render_start_card(ui, "RISK SCORE", |ui| {
                 ui.horizontal(|ui| {
                     ui.vertical(|ui| {
                         ui.label(egui::RichText::new("Low").size(28.0).strong().color(egui::Color32::from_rgb(0, 230, 118)));
                         ui.label(egui::RichText::new("2.4/10").size(11.0).color(egui::Color32::from_gray(120)));
                     });
                     
                     ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                         ui.label(egui::RichText::new("🛡").size(24.0).color(egui::Color32::from_gray(100)));
                     });
                 });
             });
        });
    });

    ui.add_space(20.0);
    
    // ---------------------------------------------------------
    // 3. MAIN SPLIT VIEW (Charts vs Live Positions)
    // ---------------------------------------------------------
    let available_height = ui.available_height() - 30.0;
    let total_width = ui.available_width();
    let gap = 15.0;
    
    // Adjust Proportions (Chart ~65%, Positions ~35%)
    let chart_width = (total_width * 0.65 - gap).max(200.0);
    let right_panel_width = total_width - chart_width - gap;
    
    ui.horizontal(|ui| {
        // --- LEFT COLUMN: CHART ---
        ui.allocate_ui_with_layout(
            egui::vec2(chart_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(22, 27, 34))
                    .rounding(10.0)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        render_chart_panel(agent, ui);
                    });
            }
        );
        
        ui.add_space(gap);
        
        // --- RIGHT COLUMN: LIVE POSITIONS ---
        ui.allocate_ui_with_layout(
            egui::vec2(right_panel_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                 // Concept Art: "LIVE POSITIONS"
                 ui.label(egui::RichText::new("LIVE POSITIONS").size(12.0).strong().color(egui::Color32::from_gray(160)));
                 ui.add_space(10.0);
                 
                 egui::ScrollArea::vertical()
                    .id_salt("live_pos_scroll")
                    .show(ui, |ui| {
                        if let Ok(pf) = agent.portfolio.try_read() {
                            for (symbol, pos) in pf.positions.iter() {
                                 // Position Card
                                 render_position_card(ui, agent, symbol, pos);
                                 ui.add_space(8.0);
                            }
                        }
                    });
            }
        );
    });
}

// --- Helpers ---

fn render_start_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(22, 27, 34))
        .rounding(10.0)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_min_height(100.0);
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).size(12.0).color(egui::Color32::from_gray(140)).strong());
            ui.add_space(8.0);
            add_contents(ui);
        });
}

fn render_position_card(ui: &mut egui::Ui, agent: &UserAgent, symbol: &str, pos: &crate::domain::trading::portfolio::Position) {
    let current_price = agent.strategy_info.get(symbol).map(|i| i.current_price).unwrap_or(pos.average_price);
    let pnl = (pos.quantity * current_price) - (pos.quantity * pos.average_price);
    let is_profit = pnl >= rust_decimal::Decimal::ZERO;
    let pnl_color = if is_profit { egui::Color32::from_rgb(0, 230, 118) } else { egui::Color32::from_rgb(255, 23, 68) }; // Neon Green/Red
    
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(28, 33, 40))
        .rounding(8.0)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
        .inner_margin(12.0)
        .show(ui, |ui| {
             ui.set_width(ui.available_width());
             
             // Header Row: Symbol + P&L Pill
             ui.horizontal(|ui| {
                 ui.label(egui::RichText::new(symbol).size(14.0).strong().color(egui::Color32::WHITE));
                 ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                      // Pill
                      egui::Frame::none()
                        .fill(pnl_color.linear_multiply(0.15))
                        .rounding(12.0)
                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                        .show(ui, |ui| {
                             ui.label(egui::RichText::new(format!("{}{:.2}", if is_profit { "+" } else { "" }, pnl))
                                .size(11.0).strong().color(pnl_color));
                        });
                 });
             });
             
             ui.add_space(8.0);
             
             // Info Grid
             ui.columns(3, |cols| {
                 cols[0].vertical(|ui| {
                     ui.label(egui::RichText::new("Size").size(10.0).color(egui::Color32::from_gray(120)));
                     ui.label(egui::RichText::new(format!("{:.4} {}", pos.quantity, symbol.split('/').next().unwrap_or(""))).size(11.0).color(egui::Color32::from_gray(200)));
                 });
                 cols[1].vertical(|ui| {
                     ui.label(egui::RichText::new("Entry").size(10.0).color(egui::Color32::from_gray(120)));
                     ui.label(egui::RichText::new(format!("${:.2}", pos.average_price)).size(11.0).color(egui::Color32::from_gray(200)));
                 });
                 cols[2].vertical(|ui| {
                     ui.label(egui::RichText::new("Current").size(10.0).color(egui::Color32::from_gray(120)));
                     ui.label(egui::RichText::new(format!("${:.2}", current_price)).size(11.0).strong().color(egui::Color32::WHITE));
                 });
             });
        });
}

/// Helper function to render a metric card (Concept Art Style)
pub fn render_metric_card(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    value: &str,
    subtitle: Option<&str>,
    value_color: egui::Color32,
    accent_color: egui::Color32,
) {
    // Standard Card Size
    let card_size = egui::vec2(190.0, 100.0);

    ui.allocate_ui_with_layout(card_size, egui::Layout::top_down(egui::Align::LEFT), |ui| {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(22, 27, 34)) // Dark Card BG
            .inner_margin(egui::Margin::same(12.0))
            .rounding(8.0)
            .shadow(egui::epaint::Shadow {
                offset: [0.0, 4.0].into(),
                blur: 16.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(100),
            })
            // Top Accent Line
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(10)))
            .show(ui, |ui| {
                ui.set_width(166.0);
                ui.set_height(76.0);

                // Row 1: Title (Left) + Icon (Right)
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(title.to_uppercase())
                            .size(10.0)
                            .color(egui::Color32::from_gray(140))
                            .strong()
                    );
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                         // Small Faded Icon
                         ui.label(egui::RichText::new(icon).size(14.0).color(accent_color.linear_multiply(0.8)));
                    });
                });
                
                ui.add_space(6.0);
                
                // Row 2: Value (Big, Center/Left)
                ui.label(
                     egui::RichText::new(value)
                         .size(24.0)
                         .strong()
                         .color(value_color)
                );
                
                // Row 3: Sparkline / Subtitle
                if let Some(sub) = subtitle {
                     ui.add_space(8.0);
                     // If it's a P&L card (indicated by color green/red), show sparkline
                     let is_pnl = value_color == egui::Color32::from_rgb(87, 171, 90) || value_color == egui::Color32::from_rgb(248, 81, 73);
                     
                     if is_pnl {
                         let is_positive = value_color == egui::Color32::from_rgb(87, 171, 90);
                         let points = if is_positive {
                             vec![
                                 egui::pos2(0.0, 15.0), egui::pos2(10.0, 12.0), egui::pos2(20.0, 14.0),
                                 egui::pos2(30.0, 8.0), egui::pos2(40.0, 10.0), egui::pos2(50.0, 2.0)
                             ]
                         } else {
                              vec![
                                 egui::pos2(0.0, 2.0), egui::pos2(10.0, 5.0), egui::pos2(20.0, 4.0),
                                 egui::pos2(30.0, 10.0), egui::pos2(40.0, 12.0), egui::pos2(50.0, 15.0)
                             ]
                         };
                         
                         ui.horizontal(|ui| {
                             let (response, painter) = ui.allocate_painter(egui::vec2(60.0, 20.0), egui::Sense::hover());
                             let to_screen = egui::emath::RectTransform::from_to(
                                 egui::Rect::from_min_size(egui::Pos2::ZERO, response.rect.size()),
                                 response.rect,
                             );
                             let screen_points: Vec<egui::Pos2> = points.iter().map(|p| to_screen.transform_pos(*p)).collect();
                             painter.add(egui::Shape::line(screen_points, egui::Stroke::new(2.0, value_color)));
                             
                             ui.label(egui::RichText::new(sub).size(10.0).color(egui::Color32::from_gray(120)));
                         });
                         
                     } else {
                         // Normal subtitle (Win Rate, Total Volume etc)
                         ui.label(egui::RichText::new(sub).size(10.0).color(egui::Color32::from_gray(120)));
                     }
                }
            });
    });
}

/// Helper function to render the activity feed (Moved from ui.rs)
pub fn render_activity_feed(
    ui: &mut egui::Ui,
    events: &VecDeque<ActivityEvent>,
) {
    egui::ScrollArea::vertical()
        .id_salt("activity_feed_scroll")
        .max_height(300.0)
        .show(ui, |ui| {
            if events.is_empty() {
                ui.label(
                    egui::RichText::new("No recent activity")
                        .color(egui::Color32::from_gray(120))
                        .italics(),
                );
            } else {
                let mut i = 0;
                for event in events {
                    let icon = match event.event_type {
                        ActivityEventType::TradeExecuted => "✅",
                        ActivityEventType::Signal => "📣",
                        ActivityEventType::FilterBlock => "⛔",
                        ActivityEventType::StrategyChange => "🔧",
                        ActivityEventType::Alert => "⚠️",
                        ActivityEventType::System => "ℹ",
                    };

                    let color = match event.severity {
                        EventSeverity::Info => egui::Color32::from_gray(200),
                        EventSeverity::Warning => egui::Color32::from_rgb(255, 212, 59),
                        EventSeverity::Error => egui::Color32::from_rgb(248, 81, 73),
                    };

                    // Striped Row Background
                    let bg_color = if i % 2 == 0 {
                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 5) // Very subtle light stripe
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    
                    egui::Frame::none()
                        .fill(bg_color)
                        .inner_margin(4.0)
                        .rounding(2.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(icon).size(12.0));
                                ui.label(
                                    egui::RichText::new(event.timestamp.format("%H:%M:%S").to_string())
                                        .size(10.0)
                                        .color(egui::Color32::from_gray(120)),
                                );
                                ui.label(
                                    egui::RichText::new(&event.message)
                                        .size(11.0)
                                        .color(color),
                                );
                            });
                        });
                    i += 1;
                }
            }
        });
}

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

        // Enhanced Tab buttons with Segmented Control look
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(agent.i18n.t("section_market"))
                    .strong()
                    .color(egui::Color32::from_gray(180)),
            );
            ui.add_space(12.0);
            
            ui.push_id("market_tabs", |ui| {
                ui.style_mut().spacing.item_spacing.x = 0.0; // Connected buttons
                
                for (idx, symbol) in symbols.iter().enumerate() {
                    let is_selected = agent.selected_chart_tab.as_ref() == Some(symbol);
                    let is_first = idx == 0;
                    let is_last = idx == symbols.len() - 1;

                    // Get trend and price info
                    let tab_label = if let Some(info) = agent.strategy_info.get(symbol) {
                        format!(
                            "{} {} ${:.2}",
                            info.trend.emoji(),
                            symbol,
                            info.current_price.to_f64().unwrap_or(0.0)
                        )
                    } else {
                        symbol.clone()
                    };

                    let rounding = if symbols.len() == 1 {
                        egui::Rounding::same(6.0)
                    } else if is_first {
                         egui::Rounding { nw: 6.0, sw: 6.0, ne: 0.0, se: 0.0 }
                    } else if is_last {
                         egui::Rounding { nw: 0.0, sw: 0.0, ne: 6.0, se: 6.0 }
                    } else {
                        egui::Rounding::ZERO
                    };

                    let button = egui::Button::new(
                        egui::RichText::new(&tab_label)
                            .size(12.0)
                            .color(if is_selected { egui::Color32::WHITE } else { egui::Color32::from_gray(170) })
                    )
                    .fill(if is_selected { egui::Color32::from_rgb(56, 139, 253) } else { egui::Color32::from_rgb(22, 27, 34) })
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                    .rounding(rounding)
                    .min_size(egui::vec2(100.0, 28.0)); // Taller tabs

                    if ui.add(button).clicked() {
                        agent.selected_chart_tab = Some(symbol.clone());
                    }
                }
            });
        });

        ui.add_space(8.0);
        
        // Chart for selected tab
        if let Some(selected_symbol) = &agent.selected_chart_tab {
            if let Some(candles) = agent.market_data.get(selected_symbol) {
                if candles.is_empty() {
                     ui.label(agent.i18n.tf("no_candles", &[("symbol", selected_symbol)]));
                } else {
                     // Info Panel
                    if let Some(strat_info) = agent.strategy_info.get(selected_symbol) {
                         egui::Frame::none()
                            .fill(egui::Color32::from_rgb(22, 27, 34))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .rounding(6.0)
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                     ui.label(egui::RichText::new("⚙️ Strategy:").strong().color(egui::Color32::from_gray(160)));
                                     
                                     let strategy_display = if strat_info.mode.to_lowercase() == "dynamicregime" {
                                         if let Some(signal) = &strat_info.last_signal {
                                             if signal.contains("Dynamic (Trend)") { "Dynamic (Trend)".to_string() }
                                             else if signal.contains("Dynamic (Choppy)") { "Dynamic (Choppy)".to_string() }
                                             else { "Dynamic".to_string() }
                                         } else { "Dynamic".to_string() }
                                     } else { strat_info.mode.clone() };

                                     ui.label(egui::RichText::new(&strategy_display).color(egui::Color32::from_rgb(88, 166, 255)));
                                     ui.separator();
                                     ui.label(egui::RichText::new(format!("SMA: {}/{}", strat_info.fast_sma, strat_info.slow_sma)).color(egui::Color32::from_gray(160)).size(11.0));
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
                                 let color = if close >= open { egui::Color32::GREEN } else { egui::Color32::RED };
                                 let min_oc = open.min(close);
                                 let max_oc = open.max(close);
                                 let mid = (open + close) / 2.0;

                                 box_elems.push(
                                     BoxElem::new(t, BoxSpread::new(low, min_oc, mid, max_oc, high))
                                     .fill(color)
                                     .stroke(egui::Stroke::new(1.0, color))
                                     .box_width(45.0)
                                 );

                                 if i >= fast_period - 1 {
                                     let fast_sum: f64 = candles[i-(fast_period-1)..=i].iter().map(|c| c.close.to_f64().unwrap_or(0.0)).sum();
                                     fast_sma_points.push([t, fast_sum/fast_period as f64]);
                                 }
                                 if i >= slow_period - 1 {
                                     let slow_sum: f64 = candles[i-(slow_period-1)..=i].iter().map(|c| c.close.to_f64().unwrap_or(0.0)).sum();
                                     slow_sma_points.push([t, slow_sum/slow_period as f64]);
                                 }
                             }
                             
                             plot_ui.box_plot(egui_plot::BoxPlot::new(box_elems).name(selected_symbol));
                             
                             if !fast_sma_points.is_empty() {
                                 plot_ui.line(egui_plot::Line::new(fast_sma_points).color(egui::Color32::from_rgb(100, 200, 255)).name("SMA 20"));
                             }
                             if !slow_sma_points.is_empty() {
                                 plot_ui.line(egui_plot::Line::new(slow_sma_points).color(egui::Color32::from_rgb(255, 165, 0)).name("SMA 50"));
                             }
                        });
                }
            }
        }
    }

}

/// Helper function to render the logs panel (Moved from ui.rs)
pub fn render_logs_panel(agent: &mut UserAgent, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("logs_panel")
        .resizable(true)
        .default_height(250.0)
        .min_height(30.0)
        .show_animated(ctx, !agent.logs_collapsed, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(agent.i18n.t("section_system_logs")).size(14.0),
                    );
                    ui.add_space(8.0);

                    // Log Level Filter Buttons
                    let filter_button =
                        |ui: &mut egui::Ui, label: &str, is_active: bool| -> bool {
                            let button = egui::Button::new(
                                egui::RichText::new(label).size(10.0).color(if is_active {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::from_gray(160)
                                }),
                            )
                            .fill(if is_active {
                                egui::Color32::from_rgb(56, 139, 253)
                            } else {
                                egui::Color32::from_rgb(33, 38, 45)
                            })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if is_active {
                                    egui::Color32::from_rgb(88, 166, 255)
                                } else {
                                    egui::Color32::from_rgb(48, 54, 61)
                                },
                            ));
                            ui.add(button).clicked()
                        };

                    if filter_button(
                        ui,
                        agent.i18n.t("filter_all"),
                        agent.log_level_filter.is_none(),
                    ) {
                        agent.log_level_filter = None;
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_info"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_info").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_info").to_string());
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_warn"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_warn").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_warn").to_string());
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_error"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_error").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_error").to_string());
                    }
                    if filter_button(
                        ui,
                        agent.i18n.t("filter_debug"),
                        agent.log_level_filter == Some(agent.i18n.t("filter_debug").to_string()),
                    ) {
                        agent.log_level_filter = Some(agent.i18n.t("filter_debug").to_string());
                    }
                });

                ui.separator();

                // Log output
                egui::ScrollArea::vertical()
                    .id_salt("logs_scroll")
                    .auto_shrink([false, true])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for (sender, msg) in &agent.chat_history {
                            // Apply log level filter
                            if let Some(ref filter_level) = agent.log_level_filter {
                                if sender == "System" {
                                    if !msg.contains(filter_level.as_str()) {
                                        continue;
                                    }
                                }
                            }

                            ui.horizontal_wrapped(|ui| {
                                let (label_text, color) = match sender.as_str() {
                                    "User" => {
                                        ("User >", egui::Color32::from_rgb(100, 200, 255))
                                    }
                                    "Agent" => {
                                        ("Agent <", egui::Color32::from_rgb(255, 200, 100))
                                    }
                                    "System" => {
                                        if msg.contains("ERROR") {
                                            ("System !", egui::Color32::from_rgb(255, 80, 80))
                                        } else if msg.contains("WARN") {
                                            ("System ?", egui::Color32::from_rgb(255, 255, 100))
                                        } else {
                                            ("System ·", egui::Color32::from_rgb(150, 150, 150))
                                        }
                                    }
                                    _ => ("Unknown", egui::Color32::GRAY),
                                };
                                ui.label(
                                    egui::RichText::new(label_text)
                                        .color(color)
                                        .strong()
                                        .size(10.0),
                                );
                                ui.label(
                                    egui::RichText::new(msg)
                                        .size(10.0)
                                        .color(egui::Color32::from_gray(200)),
                                );
                            });
                        }
                    });
            });
        });

    // Toggle button for logs (always visible at bottom)
    egui::TopBottomPanel::bottom("logs_toggle")
        .exact_height(25.0)
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(22, 27, 34))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Toggle button to show/hide logs
                let button_text = if agent.logs_collapsed {
                    agent.i18n.t("show_logs")
                } else {
                    agent.i18n.t("hide_logs")
                };
                if ui
                    .button(
                        egui::RichText::new(button_text)
                            .size(11.0)
                            .color(egui::Color32::from_gray(180)),
                    )
                    .clicked()
                {
                    agent.logs_collapsed = !agent.logs_collapsed;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} messages", agent.chat_history.len()))
                            .size(10.0)
                            .color(egui::Color32::from_gray(140)),
                    );
                });
            });
        });
}
