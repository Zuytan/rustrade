use crate::application::agents::user_agent::UserAgent;
use chrono::{TimeZone, Utc};
use eframe::egui;
use egui_plot::{BoxElem, BoxSpread, Legend, Plot};
use rust_decimal::prelude::ToPrimitive;

/// Helper function to render a metric card
fn render_metric_card(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    value: &str,
    subtitle: Option<&str>,
    value_color: egui::Color32,
) {
    // Fixed size for ALL cards - no variation
    let card_size = egui::vec2(190.0, 78.0);
    
    ui.allocate_ui_with_layout(
        card_size,
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(22, 27, 34))
                .inner_margin(egui::Margin::same(10.0))
                .rounding(6.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                .show(ui, |ui| {
                    // Force exact dimensions
                    ui.set_width(170.0);  // 190 - 20 (2x10 padding)
                    ui.set_height(58.0);  // 78 - 20 (2x10 padding)
                    
                    ui.vertical(|ui| {
                        // Icon and title in compact row
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(icon).size(14.0));
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(title)
                                    .size(10.0)
                                    .color(egui::Color32::from_gray(150)),
                            );
                        });
                        ui.add_space(4.0);
                        
                        // Value with consistent sizing
                        ui.label(
                            egui::RichText::new(value)
                                .size(18.0)
                                .strong()
                                .color(value_color),
                        );
                        
                        // Optional subtitle
                        if let Some(sub) = subtitle {
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new(sub)
                                    .size(9.0)
                                    .color(egui::Color32::from_gray(130)),
                            );
                        }
                    });
                });
        },
    );
}

/// Helper function to render the activity feed
fn render_activity_feed(ui: &mut egui::Ui, events: &std::collections::VecDeque<crate::application::agents::user_agent::ActivityEvent>) {
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
                for event in events {
                    let icon = match event.event_type {
                        crate::application::agents::user_agent::ActivityEventType::TradeExecuted => "✅",
                        crate::application::agents::user_agent::ActivityEventType::Signal => "📊",
                        crate::application::agents::user_agent::ActivityEventType::FilterBlock => "⏸️",
                        crate::application::agents::user_agent::ActivityEventType::StrategyChange => "⚙️",
                        crate::application::agents::user_agent::ActivityEventType::Alert => "⚠️",
                        crate::application::agents::user_agent::ActivityEventType::System => "ℹ️",
                    };
                    
                    let color = match event.severity {
                        crate::application::agents::user_agent::EventSeverity::Info => egui::Color32::from_gray(200),
                        crate::application::agents::user_agent::EventSeverity::Warning => egui::Color32::from_rgb(255, 212, 59),
                        crate::application::agents::user_agent::EventSeverity::Error => egui::Color32::from_rgb(248, 81, 73),
                    };
                    
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(icon).size(14.0));
                        ui.label(
                            egui::RichText::new(event.timestamp.format("%H:%M:%S").to_string())
                                .size(10.0)
                                .color(egui::Color32::from_gray(140))
                        );
                        ui.label(
                            egui::RichText::new(&event.message)
                                .size(11.0)
                                .color(color)
                        );
                    });
                    ui.add_space(4.0);
                }
            }
        });
}

impl eframe::App for UserAgent {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- 0. Enhanced Theme Configuration ---
        let mut visuals = egui::Visuals::dark();
        
        // GitHub-inspired dark theme with better contrast
        visuals.window_fill = egui::Color32::from_rgb(13, 17, 23); // GitHub dark bg
        visuals.panel_fill = egui::Color32::from_rgb(13, 17, 23);
        visuals.extreme_bg_color = egui::Color32::from_rgb(22, 27, 34); // Slightly lighter for cards
        
        // Enhanced borders and strokes
        visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)); // Subtle border
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61));
        
        // Improved text visibility
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(230));
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(200));
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(255));
        
        // Enhanced widget backgrounds
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(22, 27, 34);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(33, 38, 45);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(48, 54, 61);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(56, 139, 253); // Blue accent
        
        // Button styling
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(30, 36, 44);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(88, 166, 255));
        
        // Selection color (for tabs, etc.)
        visuals.selection.bg_fill = egui::Color32::from_rgba_premultiplied(56, 139, 253, 60);
        visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(88, 166, 255));
        
        // Enhanced shadows for depth
        visuals.window_shadow = egui::epaint::Shadow {
            offset: [0.0, 2.0].into(),
            blur: 8.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(120),
        };
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [2.0, 4.0].into(),
            blur: 12.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(140),
        };

        ctx.set_visuals(visuals);

        // --- 1. Process System Events (Logs & Candles) ---
        self.update();
        ctx.request_repaint(); // Ensure continuous updates for logs/charts

        // --- 2. Top Metric Cards Panel ---
        egui::TopBottomPanel::top("metrics_panel")
            .exact_height(90.0)
            .frame(egui::Frame::none()
                .fill(egui::Color32::from_rgb(13, 17, 23))
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            )
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                    // Calculate metrics
                    let total_value = self.calculate_total_value();
                    let (cash, position_count, unrealized_pnl, unrealized_pct) = match self.portfolio.try_read() {
                        Ok(pf) => {
                            let mut cost_basis = rust_decimal::Decimal::ZERO;
                            let mut market_value = rust_decimal::Decimal::ZERO;
                            
                            for (symbol, pos) in pf.positions.iter() {
                                let position_cost = pos.quantity * pos.average_price;
                                cost_basis += position_cost;
                                
                                if let Some(info) = self.strategy_info.get(symbol) {
                                    market_value += pos.quantity * info.current_price;
                                } else {
                                    market_value += position_cost;
                                }
                            }
                            
                            let pnl = market_value - cost_basis;
                            let pnl_pct = if cost_basis > rust_decimal::Decimal::ZERO {
                                (pnl / cost_basis * rust_decimal::Decimal::from(100)).to_f64().unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            
                            (pf.cash, pf.positions.len(), pnl, pnl_pct)
                        }
                        Err(_) => (rust_decimal::Decimal::ZERO, 0, rust_decimal::Decimal::ZERO, 0.0),
                    };
                    
                    let win_rate = self.calculate_win_rate();
                    
                    // Card 1: Total Value
                    render_metric_card(
                        ui,
                        "💰",
                        "Total Value",
                        &format!("${:.2}", total_value.to_f64().unwrap_or(0.0)),
                        None,
                        egui::Color32::from_gray(220),
                    );
                    
                    ui.add_space(8.0);
                    
                    // Card 2: Cash
                    render_metric_card(
                        ui,
                        "💵",
                        "Cash",
                        &format!("${:.2}", cash.to_f64().unwrap_or(0.0)),
                        None,
                        egui::Color32::from_rgb(87, 171, 90),
                    );
                    
                    ui.add_space(8.0);
                    
                    // Card 3: P&L Today
                    let pnl_color = if unrealized_pnl >= rust_decimal::Decimal::ZERO {
                        egui::Color32::from_rgb(87, 171, 90)
                    } else {
                        egui::Color32::from_rgb(248, 81, 73)
                    };
                    let pnl_sign = if unrealized_pnl >= rust_decimal::Decimal::ZERO { "+" } else { "" };
                    
                    render_metric_card(
                        ui,
                        "📊",
                        "P&L Today",
                        &format!("{}${:.2}", pnl_sign, unrealized_pnl.to_f64().unwrap_or(0.0).abs()),
                        Some(&format!("{}{}%", pnl_sign, format!("{:.2}", unrealized_pct.abs()))),
                        pnl_color,
                    );
                    
                    ui.add_space(8.0);
                    
                    // Card 4: Positions
                    render_metric_card(
                        ui,
                        "📈",
                        "Positions",
                        &format!("{}", position_count),
                        None,
                        egui::Color32::from_rgb(88, 166, 255),
                    );
                    
                    ui.add_space(8.0);
                    
                    // Card 5: Win Rate
                    let win_rate_color = if win_rate >= 50.0 {
                        egui::Color32::from_rgb(87, 171, 90)
                    } else if win_rate > 0.0 {
                        egui::Color32::from_rgb(255, 212, 59)
                    } else {
                        egui::Color32::from_gray(160)
                    };
                    
                    render_metric_card(
                        ui,
                        "🎯",
                        "Win Rate",
                        &format!("{:.1}%", win_rate),
                        Some(&format!("{}/{} trades", self.winning_trades, self.total_trades)),
                        win_rate_color,
                    );
                });
            });


        // --- 3. Right Info Sidebar (40%) ---
        egui::SidePanel::right("info_panel")
            .default_width(ctx.screen_rect().width() * 0.35)
            .min_width(300.0)
            .max_width(500.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Section 1: Compact Positions List
                    ui.heading(egui::RichText::new("📈 Positions").size(15.0));
                    ui.add_space(6.0);
                    
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(22, 27, 34))
                        .inner_margin(egui::Margin::same(8.0))
                        .rounding(6.0)
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("positions_scroll")
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    match self.portfolio.try_read() {
                                        Ok(pf) => {
                                            if pf.positions.is_empty() {
                                                ui.label(
                                                    egui::RichText::new("No open positions")
                                                        .color(egui::Color32::from_gray(120))
                                                        .italics(),
                                                );
                                            } else {
                                                for (symbol, pos) in &pf.positions {
                                                    let (current_price, trend_emoji) = if let Some(info) = self.strategy_info.get(symbol) {
                                                        (info.current_price, info.trend.emoji())
                                                    } else {
                                                        (pos.average_price, "➡️")
                                                    };
                                                    
                                                    let pnl = (pos.quantity * current_price) - (pos.quantity * pos.average_price);
                                                    let pnl_color = if pnl >= rust_decimal::Decimal::ZERO {
                                                        egui::Color32::from_rgb(87, 171, 90)
                                                    } else {
                                                        egui::Color32::from_rgb(248, 81, 73)
                                                    };
                                                    let pnl_sign = if pnl >= rust_decimal::Decimal::ZERO { "+" } else { "" };
                                                    
                                                    ui.horizontal(|ui| {
                                                        ui.label(egui::RichText::new(trend_emoji).size(12.0));
                                                        ui.label(
                                                            egui::RichText::new(symbol)
                                                                .strong()
                                                                .color(egui::Color32::from_rgb(255, 212, 59))
                                                                .size(12.0)
                                                        );
                                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                            ui.label(
                                                                egui::RichText::new(format!("{}${:.2}", pnl_sign, pnl.to_f64().unwrap_or(0.0).abs()))
                                                                    .strong()
                                                                    .color(pnl_color)
                                                                    .size(11.0)
                                                            );
                                                            ui.label(
                                                                egui::RichText::new(format!("${:.2}", current_price.to_f64().unwrap_or(0.0)))
                                                                    .color(egui::Color32::from_gray(180))
                                                                    .size(11.0)
                                                            );
                                                        });
                                                    });
                                                    ui.add_space(4.0);
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            ui.spinner();
                                        }
                                    }
                                });
                        });
                    
                    ui.add_space(12.0);
                    
                    // Section 2: Activity Feed
                    ui.heading(egui::RichText::new("🕒 Recent Activity").size(15.0));
                    ui.add_space(6.0);
                    
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(22, 27, 34))
                        .inner_margin(egui::Margin::same(8.0))
                        .rounding(6.0)
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                        .show(ui, |ui| {
                            render_activity_feed(ui, &self.activity_feed);
                        });
                    
                    ui.add_space(12.0);
                    
                    // Section 3: Strategy Status
                    ui.heading(egui::RichText::new("⚙️ Strategy").size(15.0));
                    ui.add_space(6.0);
                    
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(22, 27, 34))
                        .inner_margin(egui::Margin::same(10.0))
                        .rounding(6.0)
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("Mode: {}", self.strategy_mode.to_string()))
                                    .color(egui::Color32::from_rgb(88, 166, 255))
                                    .strong()
                                    .size(12.0)
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Risk Score: 6/10")
                                    .color(egui::Color32::from_gray(180))
                                    .size(11.0)
                            );
                            ui.label(
                                egui::RichText::new("SMA: 20/50")
                                    .color(egui::Color32::from_gray(180))
                                    .size(11.0)
                            );
                        });
                });
            });

        // --- 4. Enhanced Central Panel: Dashboard ---
        egui::CentralPanel::default().show(ctx, |ui| {
            // Card-style Header with Portfolio Summary
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(22, 27, 34))
                .inner_margin(egui::Margin::same(12.0))
                .rounding(6.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading(egui::RichText::new("📊 Portfolio Dashboard").size(18.0));
                        ui.add_space(10.0);
                        
                        match self.portfolio.try_read() {
                            Ok(pf) => {
                                let mut cost_basis = rust_decimal::Decimal::ZERO;
                                let mut market_value = rust_decimal::Decimal::ZERO;
                                
                                for (symbol, pos) in pf.positions.iter() {
                                    let position_cost = pos.quantity * pos.average_price;
                                    cost_basis += position_cost;
                                    
                                    // Get current price from strategy_info
                                    if let Some(info) = self.strategy_info.get(symbol) {
                                        market_value += pos.quantity * info.current_price;
                                    } else {
                                        // Fallback to average price if no current price
                                        market_value += position_cost;
                                    }
                                }
                                
                                let unrealized_pnl = market_value - cost_basis;
                                let unrealized_pct = if cost_basis > rust_decimal::Decimal::ZERO {
                                    (unrealized_pnl / cost_basis * rust_decimal::Decimal::from(100)).to_f64().unwrap_or(0.0)
                                } else {
                                    0.0
                                };
                                
                                // Push metrics to the right
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // Cash
                                    ui.label(
                                        egui::RichText::new(format!("Cash: ${:.2}", pf.cash.to_f64().unwrap_or(0.0)))
                                            .strong()
                                            .color(egui::Color32::from_rgb(87, 171, 90))
                                            .size(15.0)
                                    );
                                    ui.separator();
                                    
                                    // Value
                                    ui.label(
                                        egui::RichText::new(format!("Value: ${:.2}", market_value.to_f64().unwrap_or(0.0)))
                                            .color(egui::Color32::from_gray(200))
                                            .size(14.0)
                                    );
                                    ui.separator();
                                    
                                    // Unrealized P&L display
                                    let pnl_color = if unrealized_pnl >= rust_decimal::Decimal::ZERO {
                                        egui::Color32::from_rgb(87, 171, 90)
                                    } else {
                                        egui::Color32::from_rgb(248, 81, 73)
                                    };
                                    let pnl_sign = if unrealized_pnl >= rust_decimal::Decimal::ZERO { "+" } else { "" };
                                    
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "P&L: {}${:.2} ({}{}%)",
                                            pnl_sign,
                                            unrealized_pnl.to_f64().unwrap_or(0.0).abs(),
                                            pnl_sign,
                                            format!("{:.2}", unrealized_pct.abs())
                                        ))
                                        .strong()
                                        .color(pnl_color)
                                        .size(14.0)
                                    );
                                });
                            }
                            Err(_) => {
                                ui.spinner();
                            }
                        }
                    });
                });
            
            ui.add_space(12.0);

            match self.portfolio.try_read() {
                Ok(pf) => {
                    // Enhanced Positions Table
                    if !pf.positions.is_empty() {
                        ui.collapsing(
                            egui::RichText::new(format!("📈 Open Positions ({})", pf.positions.len()))
                                .size(15.0)
                                .strong(),
                            |ui| {
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(22, 27, 34))
                                .inner_margin(egui::Margin::same(10.0))
                                .rounding(6.0)
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                                .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(180.0)
                                    .show(ui, |ui| {
                                        egui::Grid::new("positions_grid")
                                            .striped(true)
                                            .spacing([10.0, 8.0])
                                            .show(ui, |ui| {
                                                // Enhanced Header
                                                ui.label(egui::RichText::new("SYMBOL").strong().color(egui::Color32::from_gray(160)));
                                                ui.label(egui::RichText::new("QTY").strong().color(egui::Color32::from_gray(160)));
                                                ui.label(egui::RichText::new("AVG").strong().color(egui::Color32::from_gray(160)));
                                                ui.label(egui::RichText::new("CURRENT").strong().color(egui::Color32::from_gray(160)));
                                                ui.label(egui::RichText::new("P&L $").strong().color(egui::Color32::from_gray(160)));
                                                ui.label(egui::RichText::new("P&L %").strong().color(egui::Color32::from_gray(160)));
                                                ui.label(egui::RichText::new("TREND").strong().color(egui::Color32::from_gray(160)));
                                                ui.end_row();

                                                // Rows
                                                for (symbol, pos) in &pf.positions {
                                                    // Get current price and trend from strategy_info
                                                    let (current_price, trend_emoji) = if let Some(info) = self.strategy_info.get(symbol) {
                                                        (info.current_price, info.trend.emoji())
                                                    } else {
                                                        (pos.average_price, "➡️")
                                                    };
                                                    
                                                    let cost_basis = pos.quantity * pos.average_price;
                                                    let market_value = pos.quantity * current_price;
                                                    let pnl = market_value - cost_basis;
                                                    let pnl_pct = if cost_basis > rust_decimal::Decimal::ZERO {
                                                        (pnl / cost_basis * rust_decimal::Decimal::from(100)).to_f64().unwrap_or(0.0)
                                                    } else {
                                                        0.0
                                                    };
                                                    
                                                    // P&L color
                                                    let pnl_color = if pnl >= rust_decimal::Decimal::ZERO {
                                                        egui::Color32::from_rgb(87, 171, 90)
                                                    } else {
                                                        egui::Color32::from_rgb(248, 81, 73)
                                                    };
                                                    let pnl_sign = if pnl >= rust_decimal::Decimal::ZERO { "+" } else { "" };
                                                    
                                                    // Symbol (fixed width)
                                                    ui.add_sized([90.0, 20.0], egui::Label::new(
                                                        egui::RichText::new(symbol)
                                                            .strong()
                                                            .color(egui::Color32::from_rgb(255, 212, 59))
                                                    ));
                                                    // Quantity (fixed width)
                                                    ui.add_sized([70.0, 20.0], egui::Label::new(
                                                        egui::RichText::new(format!("{:.4}", pos.quantity.to_f64().unwrap_or(0.0)))
                                                            .color(egui::Color32::from_gray(200))
                                                    ));
                                                    // Average price (fixed width)
                                                    ui.add_sized([70.0, 20.0], egui::Label::new(
                                                        egui::RichText::new(format!("${:.2}", pos.average_price.to_f64().unwrap_or(0.0)))
                                                            .color(egui::Color32::from_gray(180))
                                                    ));
                                                    // Current price (fixed width)
                                                    ui.add_sized([70.0, 20.0], egui::Label::new(
                                                        egui::RichText::new(format!("${:.2}", current_price.to_f64().unwrap_or(0.0)))
                                                            .strong()
                                                            .color(egui::Color32::from_gray(220))
                                                    ));
                                                    // P&L $ (fixed width)
                                                    ui.add_sized([80.0, 20.0], egui::Label::new(
                                                        egui::RichText::new(format!("{}${:.2}", pnl_sign, pnl.to_f64().unwrap_or(0.0).abs()))
                                                            .strong()
                                                            .color(pnl_color)
                                                    ));
                                                    // P&L % (fixed width)
                                                    ui.add_sized([80.0, 20.0], egui::Label::new(
                                                        egui::RichText::new(format!("{}{}%", pnl_sign, format!("{:.2}", pnl_pct.abs())))
                                                            .strong()
                                                            .color(pnl_color)
                                                    ));
                                                    // Trend (fixed width)
                                                    ui.add_sized([50.0, 20.0], egui::Label::new(
                                                        egui::RichText::new(trend_emoji).size(14.0)
                                                    ));
                                                    ui.end_row();
                                                }
                                            });
                                });
                            });
                        });
                        ui.add_space(12.0);
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("📈 Open Positions:").size(15.0).strong());
                            ui.label(egui::RichText::new("None").italics().color(egui::Color32::from_gray(120)));
                        });
                        ui.add_space(12.0);
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

                // Enhanced Tab buttons with better styling
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📊 Market:").strong().color(egui::Color32::from_gray(180)));
                    ui.add_space(8.0);
                    for symbol in &symbols {
                        let is_selected = self.selected_chart_tab.as_ref() == Some(symbol);
                        
                        // Get trend and price info
                        let tab_label = if let Some(info) = self.strategy_info.get(symbol) {
                            format!("{} {} ${:.2}", 
                                info.trend.emoji(), 
                                symbol, 
                                info.current_price.to_f64().unwrap_or(0.0)
                            )
                        } else {
                            symbol.clone()
                        };
                        
                        let button = egui::Button::new(
                            egui::RichText::new(&tab_label)
                                .size(12.0)
                                .color(if is_selected { egui::Color32::WHITE } else { egui::Color32::from_gray(180) })
                        )
                        .fill(if is_selected { 
                            egui::Color32::from_rgb(56, 139, 253) 
                        } else { 
                            egui::Color32::from_rgb(22, 27, 34) 
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if is_selected { 
                                egui::Color32::from_rgb(88, 166, 255) 
                            } else { 
                                egui::Color32::from_rgb(48, 54, 61) 
                            }
                        ));
                        
                        if ui.add(button).clicked() {
                            self.selected_chart_tab = Some(symbol.clone());
                        }
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Chart for selected tab
                if let Some(selected_symbol) = &self.selected_chart_tab {
                    if let Some(candles) = self.market_data.get(selected_symbol) {
                        if candles.is_empty() {
                            ui.label(format!("No candles yet for {}", selected_symbol));
                        } else {
                            // Enhanced Strategy Info Panel
                            if let Some(strat_info) = self.strategy_info.get(selected_symbol) {
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(22, 27, 34))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .rounding(6.0)
                                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                                    .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("⚙️ Strategy:").strong().color(egui::Color32::from_gray(160)));
                                    
                                    // If dynamic strategy, try to extract current regime from last signal
                                    let strategy_display = if strat_info.mode.to_lowercase() == "dynamicregime" {
                                        if let Some(signal) = &strat_info.last_signal {
                                            // Extract regime from signal reason
                                            if signal.contains("Dynamic (Trend)") {
                                                "Dynamic (Trend)".to_string()
                                            } else if signal.contains("Dynamic (Choppy)") {
                                                "Dynamic (Choppy)".to_string()
                                            } else {
                                                "Dynamic".to_string()
                                            }
                                        } else {
                                            "Dynamic".to_string()
                                        }
                                    } else {
                                        strat_info.mode.clone()
                                    };
                                    
                                    ui.label(
                                        egui::RichText::new(&strategy_display)
                                            .color(egui::Color32::from_rgb(88, 166, 255))
                                    );
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "SMA: {}/{}",
                                            strat_info.fast_sma as i32, strat_info.slow_sma as i32
                                        ))
                                        .color(egui::Color32::from_gray(160))
                                        .size(11.0),
                                    );
                                    if let Some(signal) = &strat_info.last_signal {
                                        ui.separator();
                                        let signal_color = if signal.contains("Buy")
                                            || signal.contains("Golden")
                                        {
                                            egui::Color32::from_rgb(87, 171, 90)
                                        } else {
                                            egui::Color32::from_rgb(248, 81, 73)
                                        };
                                        ui.label(
                                            egui::RichText::new(signal)
                                                .color(signal_color)
                                                .size(11.0)
                                                .strong(),
                                        );
                                    }
                                });
                                });
                                ui.add_space(6.0);
                            }

                            ui.label(
                                egui::RichText::new(format!(
                                    "📊 {} ({} candles)",
                                    selected_symbol,
                                    candles.len()
                                ))
                                .size(14.0)
                                .color(egui::Color32::from_gray(180))
                            );
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

        // --- 5. Collapsible Bottom Logs Panel ---
        egui::TopBottomPanel::bottom("logs_panel")
            .resizable(true)
            .default_height(250.0)
            .min_height(30.0)
            .show_animated(ctx, !self.logs_collapsed, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading(egui::RichText::new("📋 System Logs").size(14.0));
                        ui.add_space(8.0);
                        
                        // Log Level Filter Buttons
                        let filter_button = |ui: &mut egui::Ui, label: &str, is_active: bool| -> bool {
                            let button = egui::Button::new(
                                egui::RichText::new(label)
                                    .size(10.0)
                                    .color(if is_active { egui::Color32::WHITE } else { egui::Color32::from_gray(160) })
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
                                }
                            ));
                            ui.add(button).clicked()
                        };
                        
                        if filter_button(ui, "All", self.log_level_filter.is_none()) {
                            self.log_level_filter = None;
                        }
                        if filter_button(ui, "INFO", self.log_level_filter == Some("INFO".to_string())) {
                            self.log_level_filter = Some("INFO".to_string());
                        }
                        if filter_button(ui, "WARN", self.log_level_filter == Some("WARN".to_string())) {
                            self.log_level_filter = Some("WARN".to_string());
                        }
                        if filter_button(ui, "ERROR", self.log_level_filter == Some("ERROR".to_string())) {
                            self.log_level_filter = Some("ERROR".to_string());
                        }
                        if filter_button(ui, "DEBUG", self.log_level_filter == Some("DEBUG".to_string())) {
                            self.log_level_filter = Some("DEBUG".to_string());
                        }
                    });
                    
                    ui.separator();
                    
                    // Log output
                    egui::ScrollArea::vertical()
                        .id_salt("logs_scroll")
                        .auto_shrink([false, true])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for (sender, msg) in &self.chat_history {
                                // Apply log level filter
                                if let Some(ref filter_level) = self.log_level_filter {
                                    if sender == "System" {
                                        if !msg.contains(filter_level.as_str()) {
                                            continue;
                                        }
                                    }
                                }

                                ui.horizontal_wrapped(|ui| {
                                    let (label_text, color) = match sender.as_str() {
                                        "User" => ("User >", egui::Color32::from_rgb(100, 200, 255)),
                                        "Agent" => ("Agent <", egui::Color32::from_rgb(255, 200, 100)),
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
                                    ui.label(egui::RichText::new(label_text).color(color).strong().size(10.0));
                                    ui.label(egui::RichText::new(msg).size(10.0).color(egui::Color32::from_gray(200)));
                                });
                            }
                        });
                });
            });
        
        // Toggle button for logs (always visible at bottom)
        egui::TopBottomPanel::bottom("logs_toggle")
            .exact_height(25.0)
            .frame(egui::Frame::none()
                .fill(egui::Color32::from_rgb(22, 27, 34))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let toggle_text = if self.logs_collapsed { "▲ Show Logs" } else { "▼ Hide Logs" };
                    if ui.button(
                        egui::RichText::new(toggle_text)
                            .size(11.0)
                            .color(egui::Color32::from_gray(180))
                    ).clicked() {
                        self.logs_collapsed = !self.logs_collapsed;
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{} messages", self.chat_history.len()))
                                .size(10.0)
                                .color(egui::Color32::from_gray(140))
                        );
                    });
                });
            });

        // Force frequent repaints to ensure responsive logs
        ctx.request_repaint();
    }
}
