use crate::domain::performance::monte_carlo::MonteCarloResult;
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
    let (_cash, position_count, unrealized_pnl, unrealized_pct, market_value) = match agent.portfolio.try_read() {
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
                    egui::RichText::new(agent.i18n.tf("total_value_format", &[("amount", &format!("{:.2}", total_value.to_f64().unwrap_or(0.0)))]))
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
                             egui::RichText::new(agent.i18n.tf("pnl_pill_format", &[
                                 ("amount", &format!("{:.2}", unrealized_pnl.to_f64().unwrap_or(0.0).abs())),
                                 ("percent", &format!("{:.2}", unrealized_pct)),
                                 ("sign", &pnl_sign.to_string())
                             ]))
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
                         egui::RichText::new(agent.i18n.tf("status_label", &[("status", &agent.i18n.t("status_active"))]))
                             .size(12.0)
                             .color(egui::Color32::from_gray(160))
                     );
                     ui.add_space(10.0);
                     ui.label(
                         egui::RichText::new(agent.i18n.tf("latency_label", &[("ms", &agent.latency_ms.to_string())]))
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
        let pnl_arrow = if pnl_val >= 0.0 { "↗" } else { "↘" };
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
                     ui.label(egui::RichText::new(agent.i18n.t("metric_daily_pnl")).size(12.0).color(egui::Color32::from_gray(140)).strong());
                     ui.add_space(8.0);
                     
                     ui.horizontal(|ui| {
                         let sign = if pnl_val >= 0.0 { "+" } else { "-" };
                         ui.label(egui::RichText::new(agent.i18n.tf("pnl_value_format", &[
                             ("amount", &format!("{:.2}", pnl_val.abs())),
                             ("sign", &sign.to_string())
                         ])).size(28.0).strong().color(pnl_color));
                         ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                             ui.label(egui::RichText::new(pnl_arrow).size(18.0).color(pnl_color));
                         });
                     });
                     ui.add_space(4.0);
                     ui.label(egui::RichText::new(agent.i18n.t("last_24h")).size(11.0).color(egui::Color32::from_gray(100)));
                });
        });

        // Card 2: WIN RATE (Circle)
        columns[1].push_id("card_win_rate", |ui| {
             render_start_card(ui, agent.i18n.t("metric_win_rate"), |ui| {
                 ui.horizontal(|ui| {
                     // Text
                     ui.vertical(|ui| {
                         ui.label(egui::RichText::new(agent.i18n.tf("percent_format", &[("value", &format!("{:.1}", win_rate))])).size(28.0).strong().color(egui::Color32::from_rgb(56, 139, 253)));
                         ui.label(egui::RichText::new(agent.i18n.tf("trades_count_format", &[
                             ("winning", &agent.winning_trades.to_string()),
                             ("total", &agent.total_trades.to_string())
                         ])).size(11.0).color(egui::Color32::from_gray(120)));
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
             render_start_card(ui, agent.i18n.t("metric_open_positions"), |ui| {
                 ui.horizontal(|ui| {
                     ui.vertical(|ui| {
                         ui.label(egui::RichText::new(format!("{}", position_count)).size(28.0).strong().color(egui::Color32::WHITE));
                         ui.label(egui::RichText::new(agent.i18n.tf("total_volume_format", &[("amount", &format!("{:.0}", market_value.to_f64().unwrap_or(0.0)))]))
                             .size(11.0).color(egui::Color32::from_gray(120)));
                     });
                     
                     ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                         ui.label(egui::RichText::new("🪙").size(24.0).color(egui::Color32::from_gray(100)));
                     });
                 });
             });
        });

        // Card 4: RISK SCORE (Shield)
        columns[3].push_id("card_risk", |ui| {
             render_start_card(ui, agent.i18n.t("metric_risk_score"), |ui| {
                 ui.horizontal(|ui| {
                     ui.vertical(|ui| {
                         ui.label(egui::RichText::new(agent.i18n.t("risk_low")).size(28.0).strong().color(egui::Color32::from_rgb(0, 230, 118)));
                         ui.label(egui::RichText::new(agent.i18n.tf("risk_score_label_short", &[("score", &agent.risk_score.to_string())])).size(11.0).color(egui::Color32::from_gray(120)));
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
        
        // --- RIGHT COLUMN: MARKET & POSITIONS ---
        ui.allocate_ui_with_layout(
            egui::vec2(right_panel_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                 ui.label(egui::RichText::new(agent.i18n.t("market_and_positions")).size(12.0).strong().color(egui::Color32::from_gray(160)));
                 ui.add_space(10.0);
                 
                 egui::ScrollArea::vertical()
                    .id_salt("market_list_scroll")
                    .show(ui, |ui| {
                        let mut symbols: Vec<_> = agent.market_data.keys().cloned().collect();
                        symbols.sort();

                         if let Ok(pf) = agent.portfolio.try_read() {
                            for symbol in symbols {
                                 let pos = pf.positions.get(&symbol);
                                 let is_selected = agent.selected_chart_tab.as_ref() == Some(&symbol);
                                 
                                 if render_symbol_card(ui, agent, &symbol, pos, is_selected).clicked() {
                                     agent.selected_chart_tab = Some(symbol.clone());
                                 }
                                 ui.add_space(8.0);
                            }
                        }
                    });

                 ui.add_space(20.0);
                 ui.label(egui::RichText::new(agent.i18n.t("section_recent_activity")).size(12.0).strong().color(egui::Color32::from_gray(160)));
                 ui.add_space(10.0);
                 
                 render_activity_feed(ui, &agent.activity_feed, &agent.i18n);
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

fn render_symbol_card(
    ui: &mut egui::Ui, 
    agent: &UserAgent, 
    symbol: &str, 
    pos: Option<&crate::domain::trading::portfolio::Position>,
    is_selected: bool
) -> egui::Response {
    let current_price = agent.strategy_info.get(symbol).map(|i| i.current_price).unwrap_or(
        pos.map(|p| p.average_price).unwrap_or(rust_decimal::Decimal::ZERO)
    );
    
    let frame = if is_selected {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(28, 33, 40))
            .rounding(8.0)
            .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(41, 121, 255))) // Blue Active Stroke
            .shadow(egui::epaint::Shadow {
                offset: [0.0, 2.0].into(),
                blur: 10.0,
                spread: 0.0,
                color: egui::Color32::from_rgba_premultiplied(41, 121, 255, 25), // Blue Glow
            })
            .inner_margin(12.0)
    } else {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(28, 33, 40))
            .rounding(8.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
            .inner_margin(12.0)
    };

    let response = frame.show(ui, |ui| {
         ui.set_width(ui.available_width());
         
         // Header Row: Symbol + P&L or Trend
         ui.horizontal(|ui| {
             ui.label(egui::RichText::new(symbol).size(14.0).strong().color(egui::Color32::WHITE));
             
             ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                  if let Some(pos) = pos {
                       let pnl = (pos.quantity * current_price) - (pos.quantity * pos.average_price);
                       let is_profit = pnl >= rust_decimal::Decimal::ZERO;
                       let pnl_color = if is_profit { egui::Color32::from_rgb(0, 230, 118) } else { egui::Color32::from_rgb(255, 23, 68) };
                       
                       egui::Frame::none()
                        .fill(pnl_color.linear_multiply(0.15))
                        .rounding(12.0)
                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                        .show(ui, |ui| {
                             ui.label(egui::RichText::new(agent.i18n.tf("pnl_amount_format", &[
                                 ("amount", &format!("{:.2}", pnl.to_f64().unwrap_or(0.0).abs())),
                                 ("sign", &if is_profit { "+" } else { "-" }.to_string())
                             ]))
                                .size(11.0).strong().color(pnl_color));
                        });
                  } else if let Some(info) = agent.strategy_info.get(symbol) {
                       ui.label(egui::RichText::new(info.trend.emoji()).size(14.0));
                  }
             });
         });
         
         ui.add_space(4.0);
         
         if let Some(pos) = pos {
             // Position Info Grid
             ui.columns(3, |cols| {
                 cols[0].vertical(|ui| {
                     ui.label(egui::RichText::new(agent.i18n.t("header_quantity")).size(10.0).color(egui::Color32::from_gray(120)));
                     ui.label(egui::RichText::new(format!("{:.4}", pos.quantity)).size(11.0).color(egui::Color32::from_gray(200)));
                 });
                 cols[1].vertical(|ui| {
                     ui.label(egui::RichText::new(agent.i18n.t("header_average")).size(10.0).color(egui::Color32::from_gray(120)));
                     ui.label(egui::RichText::new(agent.i18n.tf("currency_format", &[("amount", &format!("{:.2}", pos.average_price))]))
                         .size(11.0).color(egui::Color32::from_gray(200)));
                 });
                 cols[2].vertical(|ui| {
                     ui.label(egui::RichText::new(agent.i18n.t("header_current")).size(10.0).color(egui::Color32::from_gray(120)));
                     ui.label(egui::RichText::new(agent.i18n.tf("currency_format", &[("amount", &format!("{:.2}", current_price))]))
                         .size(11.0).strong().color(egui::Color32::WHITE));
                 });
             });
         } else {
             // Watchlist Info (Single Row)
             ui.horizontal(|ui| {
                 ui.label(egui::RichText::new(agent.i18n.t("header_current")).size(10.0).color(egui::Color32::from_gray(120)));
                 ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                     ui.label(egui::RichText::new(agent.i18n.tf("currency_format", &[("amount", &format!("{:.2}", current_price))]))
                         .size(12.0).strong().color(egui::Color32::WHITE));
                 });
             });
         }
    }).response;

    ui.interact(response.rect, response.id, egui::Sense::click())
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
    i18n: &crate::domain::ui::I18nService,
) {
    egui::ScrollArea::vertical()
        .id_salt("activity_feed_scroll")
        .max_height(300.0)
        .show(ui, |ui| {
            if events.is_empty() {
                ui.label(
                    egui::RichText::new(i18n.t("no_activity"))
                        .color(egui::Color32::from_gray(120))
                        .italics(),
                );
            } else {
                for (i, event) in events.iter().enumerate() {
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

        // --- Selection moved to right panel (no tabs here anymore) ---
        ui.horizontal(|ui| {
            if let Some(selected_symbol) = &agent.selected_chart_tab {
                 ui.label(
                    egui::RichText::new(agent.i18n.tf("live_market_format", &[("symbol", selected_symbol)]))
                        .strong()
                        .size(16.0)
                        .color(egui::Color32::WHITE),
                );
            }
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
                                     ui.label(egui::RichText::new(agent.i18n.t("strategy_label")).strong().color(egui::Color32::from_gray(160)));
                                     
                                     let strategy_display = if strat_info.mode.to_lowercase() == "dynamicregime" {
                                         if let Some(signal) = &strat_info.last_signal {
                                             if signal.contains("Dynamic (Trend)") { agent.i18n.t("dynamic_trend").to_string() }
                                             else if signal.contains("Dynamic (Choppy)") { agent.i18n.t("dynamic_choppy").to_string() }
                                             else { agent.i18n.t("dynamic").to_string() }
                                         } else { agent.i18n.t("dynamic").to_string() }
                                     } else { strat_info.mode.clone() };

                                     ui.label(egui::RichText::new(&strategy_display).color(egui::Color32::from_rgb(88, 166, 255)));
                                     ui.separator();
                                     ui.label(egui::RichText::new(agent.i18n.tf("sma_label", &[
                                         ("fast", &strat_info.fast_sma.to_string()),
                                         ("slow", &strat_info.slow_sma.to_string())
                                     ])).color(egui::Color32::from_gray(160)).size(11.0));
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
                                 plot_ui.line(egui_plot::Line::new(fast_sma_points).color(egui::Color32::from_rgb(100, 200, 255)).name(agent.i18n.t("sma_20_label")));
                             }
                             if !slow_sma_points.is_empty() {
                                 plot_ui.line(egui_plot::Line::new(slow_sma_points).color(egui::Color32::from_rgb(255, 165, 0)).name(agent.i18n.t("sma_50_label")));
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
                                // Check if sender is a system message (matches any of the system sender keys)
                                let is_system = sender == agent.i18n.t("sender_system") 
                                    || sender == agent.i18n.t("sender_system_error") 
                                    || sender == agent.i18n.t("sender_system_warn");
                                if is_system {
                                    if !msg.contains(filter_level.as_str()) {
                                        continue;
                                    }
                                }
                            }

                            ui.horizontal_wrapped(|ui| {
                                let (label_key, color) = match sender.as_str() {
                                    s if s == agent.i18n.t("sender_user") => {
                                        ("sender_user", egui::Color32::from_rgb(100, 200, 255))
                                    }
                                    s if s == agent.i18n.t("sender_agent") => {
                                        ("sender_agent", egui::Color32::from_rgb(255, 200, 100))
                                    }
                                    _ => {
                                        if msg.contains("ERROR") {
                                            ("sender_system_error", egui::Color32::from_rgb(255, 80, 80))
                                        } else if msg.contains("WARN") {
                                            ("sender_system_warn", egui::Color32::from_rgb(255, 255, 100))
                                        } else {
                                            ("sender_system", egui::Color32::from_rgb(150, 150, 150))
                                        }
                                    }
                                };
                                ui.label(
                                    egui::RichText::new(agent.i18n.t(label_key))
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
                        egui::RichText::new(agent.i18n.tf("messages_count", &[("count", &agent.chat_history.len().to_string())]))
                            .size(10.0)
                            .color(egui::Color32::from_gray(140)),
                    );
                });
            });
        });
}

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
                                    let config = crate::domain::performance::monte_carlo::MonteCarloConfig {
                                        iterations: 10000,
                                        steps: 100,
                                        initial_equity: agent.calculate_total_value(),
                                        win_rate: agent.calculate_win_rate() / 100.0,
                                        avg_win_pct: 0.02, // Mock for now, should come from metrics
                                        avg_loss_pct: 0.015,
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

fn render_mini_metric(ui: &mut egui::Ui, label: String, value: &str, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label.to_uppercase()).size(9.0).color(egui::Color32::from_gray(120)));
        ui.label(egui::RichText::new(value).size(16.0).strong().color(color));
    });
}

fn render_correlation_heatmap(ui: &mut egui::Ui, agent: &UserAgent) {
    let mut symbols: Vec<_> = agent.market_data.keys().cloned().collect();
    symbols.sort();
    
    if symbols.is_empty() {
        ui.label(egui::RichText::new("Waiting for market data symbols...").italics().color(egui::Color32::from_gray(100)));
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
                        let corr = agent.correlation_matrix.get(&(s1.clone(), s2.clone())).cloned().unwrap_or(0.0);
                        
                        // Color mapping: Red (-1) -> Black (0) -> Green (1)
                        let color = if corr > 0.0 {
                            egui::Color32::from_rgb(0, (230.0 * corr) as u8, 118).linear_multiply(0.5 + (0.5 * corr) as f32)
                        } else {
                            egui::Color32::from_rgb((255.0 * corr.abs()) as u8, 23, 68).linear_multiply(0.5 + (0.5 * corr.abs()) as f32)
                        };

                        let (rect, _response) = ui.allocate_exact_size(egui::vec2(cell_size, cell_size), egui::Sense::hover());
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
