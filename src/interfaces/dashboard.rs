use crate::application::agents::user_agent::UserAgent;
use crate::interfaces::dashboard_components::{
    activity_feed::render_activity_feed, chart_panel::render_chart_panel,
    news_feed::render_news_feed,
};

use eframe::egui;

use rust_decimal::prelude::ToPrimitive;

/// Renders the main Dashboard content (Concept Art Layout)
pub fn render_dashboard(ui: &mut egui::Ui, agent: &mut UserAgent) {
    // --- Data Prep ---
    let total_value = agent.calculate_total_value();
    let (_cash, position_count, unrealized_pnl, unrealized_pct, market_value) =
        match agent.portfolio.try_read() {
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
                    (pnl / cost_basis * rust_decimal::Decimal::from(100))
                        .to_f64()
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                (pf.cash, pf.positions.len(), pnl, pnl_pct, mv)
            }
            Err(_) => (
                rust_decimal::Decimal::ZERO,
                0,
                rust_decimal::Decimal::ZERO,
                0.0,
                rust_decimal::Decimal::ZERO,
            ),
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
                    egui::RichText::new(agent.i18n.tf(
                        "total_value_format",
                        &[(
                            "amount",
                            &format!("{:.2}", total_value.to_f64().unwrap_or(0.0)),
                        )],
                    ))
                    .size(28.0)
                    .strong()
                    .color(egui::Color32::WHITE),
                );

                ui.add_space(10.0);

                // Small P&L Pill
                let pnl_color = if unrealized_pnl >= rust_decimal::Decimal::ZERO {
                    egui::Color32::from_rgb(0, 230, 118) // Neon Green
                } else {
                    egui::Color32::from_rgb(255, 23, 68) // Neon Red
                };
                let pnl_sign = if unrealized_pnl >= rust_decimal::Decimal::ZERO {
                    "+"
                } else {
                    ""
                };

                egui::Frame::NONE
                    .fill(pnl_color.linear_multiply(0.15))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "pnl_pill_format",
                                &[
                                    (
                                        "amount",
                                        &format!(
                                            "{:.2}",
                                            unrealized_pnl.to_f64().unwrap_or(0.0).abs()
                                        ),
                                    ),
                                    ("percent", &format!("{:.2}", unrealized_pct)),
                                    ("sign", pnl_sign),
                                ],
                            ))
                            .size(12.0)
                            .strong()
                            .color(pnl_color),
                        );
                    });
            });
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // System Status
            ui.group(|ui| {
                ui.set_style(ui.style().clone()); // Reset style
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("●")
                            .size(10.0)
                            .color(egui::Color32::GREEN),
                    );
                    ui.label(
                        egui::RichText::new(
                            agent
                                .i18n
                                .tf("status_label", &[("status", agent.i18n.t("status_active"))]),
                        )
                        .size(12.0)
                        .color(egui::Color32::from_gray(160)),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(
                            agent
                                .i18n
                                .tf("latency_label", &[("ms", &agent.latency_ms.to_string())]),
                        )
                        .size(12.0)
                        .color(egui::Color32::from_gray(100)),
                    );
                });
            });
        });
    });

    ui.add_space(20.0);

    // ---------------------------------------------------------
    // 2. METRICS CARDS (5 Columns)
    // ---------------------------------------------------------
    ui.columns(5, |columns| {
        // Card 1: DAILY P&L (Active Blue Border Effect)
        let pnl_val = unrealized_pnl.to_f64().unwrap_or(0.0);
        let pnl_color = if pnl_val >= 0.0 {
            egui::Color32::from_rgb(0, 230, 118)
        } else {
            egui::Color32::from_rgb(255, 23, 68)
        };
        let pnl_arrow = if pnl_val >= 0.0 { "↗" } else { "↘" };
        columns[0].push_id("card_daily_pnl", |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_rgb(22, 27, 34))
                .corner_radius(10)
                .stroke(egui::Stroke::new(
                    1.5,
                    egui::Color32::from_rgb(41, 121, 255),
                )) // Blue Active Stroke
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 15,
                    spread: 0,
                    color: egui::Color32::from_rgba_premultiplied(41, 121, 255, 40), // Blue Glow
                })
                .inner_margin(16)
                .show(ui, |ui| {
                    ui.set_min_height(100.0);
                    ui.label(
                        egui::RichText::new(agent.i18n.t("metric_daily_pnl"))
                            .size(12.0)
                            .color(egui::Color32::from_gray(140))
                            .strong(),
                    );
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        let sign = if pnl_val >= 0.0 { "+" } else { "-" };
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "pnl_value_format",
                                &[("amount", &format!("{:.2}", pnl_val.abs())), ("sign", sign)],
                            ))
                            .size(28.0)
                            .strong()
                            .color(pnl_color),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            ui.label(egui::RichText::new(pnl_arrow).size(18.0).color(pnl_color));
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(agent.i18n.t("last_24h"))
                            .size(11.0)
                            .color(egui::Color32::from_gray(100)),
                    );
                });
        });

        // Card 2: WIN RATE (Circle)
        columns[1].push_id("card_win_rate", |ui| {
            render_start_card(ui, agent.i18n.t("metric_win_rate"), |ui| {
                ui.horizontal(|ui| {
                    // Text
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(
                                agent.i18n.tf(
                                    "percent_format",
                                    &[("value", &format!("{:.1}", win_rate))],
                                ),
                            )
                            .size(28.0)
                            .strong()
                            .color(egui::Color32::from_rgb(56, 139, 253)),
                        );
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "trades_count_format",
                                &[
                                    ("winning", &agent.winning_trades.to_string()),
                                    ("total", &agent.total_trades.to_string()),
                                ],
                            ))
                            .size(11.0)
                            .color(egui::Color32::from_gray(120)),
                        );
                    });

                    // Donut Chart (Simulated)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                        ui.painter().circle_stroke(
                            rect.center(),
                            18.0,
                            egui::Stroke::new(4.0, egui::Color32::from_gray(40)),
                        ); // Track

                        // Arc based on actual win rate
                        let center = rect.center();
                        let radius = 18.0;
                        let start_angle = -90.0_f32.to_radians(); // Start from top
                        let sweep_angle = (360.0 * (win_rate / 100.0)) as f32;
                        let _end_angle = start_angle + sweep_angle.to_radians();

                        // Helper to get point on circle
                        let get_point = |angle: f32| -> egui::Pos2 {
                            egui::pos2(
                                center.x + radius * angle.cos(),
                                center.y + radius * angle.sin(),
                            )
                        };

                        // Draw arc path
                        if win_rate > 0.0 {
                            use egui::epaint::PathShape;
                            // Approximate arc with lines (simple way)
                            let mut points = Vec::new();
                            let steps = 32;
                            for i in 0..=steps {
                                let t = i as f32 / steps as f32;
                                let angle = start_angle + t * sweep_angle.to_radians();
                                points.push(get_point(angle));
                            }

                            ui.painter().add(PathShape::line(
                                points,
                                egui::Stroke::new(4.0, egui::Color32::from_rgb(56, 139, 253)),
                            ));
                        }
                    });
                });
            });
        });

        // Card 3: OPEN POSITIONS (Icon)
        columns[2].push_id("card_open_pos", |ui| {
            render_start_card(ui, agent.i18n.t("metric_open_positions"), |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{}", position_count))
                                .size(28.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        );
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "total_volume_format",
                                &[(
                                    "amount",
                                    &format!("{:.0}", market_value.to_f64().unwrap_or(0.0)),
                                )],
                            ))
                            .size(11.0)
                            .color(egui::Color32::from_gray(120)),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("🪙")
                                .size(24.0)
                                .color(egui::Color32::from_gray(100)),
                        );
                    });
                });
            });
        });

        // Card 4: RISK SCORE (Shield)
        columns[3].push_id("card_risk", |ui| {
            render_start_card(ui, agent.i18n.t("metric_risk_score"), |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let (label_key, color) = match agent.risk_score {
                            1..=3 => ("risk_low", egui::Color32::from_rgb(0, 230, 118)), // Green
                            4..=7 => ("risk_medium", egui::Color32::from_rgb(255, 212, 59)), // Yellow
                            _ => ("risk_high", egui::Color32::from_rgb(255, 23, 68)),        // Red
                        };
                        ui.label(
                            egui::RichText::new(agent.i18n.t(label_key))
                                .size(28.0)
                                .strong()
                                .color(color),
                        );
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "risk_score_label_short",
                                &[("score", &agent.risk_score.to_string())],
                            ))
                            .size(11.0)
                            .color(egui::Color32::from_gray(120)),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("🛡")
                                .size(24.0)
                                .color(egui::Color32::from_gray(100)),
                        );
                    });
                });
            });
        });

        // Card 5: MARKET MOOD (Brain/Thermometer)
        columns[4].push_id("card_market_mood", |ui| {
            render_start_card(ui, "MARKET MOOD", |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        if let Some(sentiment) = &agent.market_sentiment {
                            let color =
                                egui::Color32::from_hex(sentiment.classification.color_hex())
                                    .unwrap_or(egui::Color32::GRAY);

                            ui.label(
                                egui::RichText::new(sentiment.classification.to_string())
                                    .size(22.0)
                                    .strong()
                                    .color(color),
                            );

                            // Progress Bar / Gauge representation
                            let (rect, _resp) =
                                ui.allocate_at_least(egui::vec2(100.0, 6.0), egui::Sense::hover());
                            ui.painter()
                                .rect_filled(rect, 3.0, egui::Color32::from_gray(40));

                            let progress_width = 100.0 * (sentiment.value as f32 / 100.0);
                            let progress_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(progress_width, 6.0),
                            );
                            ui.painter().rect_filled(progress_rect, 3.0, color);

                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("Index: {}", sentiment.value))
                                    .size(11.0)
                                    .color(egui::Color32::from_gray(120)),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("Loading...")
                                    .size(22.0)
                                    .strong()
                                    .color(egui::Color32::GRAY),
                            );
                            ui.label(
                                egui::RichText::new("Waiting for data")
                                    .size(11.0)
                                    .color(egui::Color32::from_gray(120)),
                            );
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("🌡")
                                .size(24.0)
                                .color(egui::Color32::from_gray(100)),
                        );
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
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(22, 27, 34))
                    .corner_radius(10)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        render_chart_panel(agent, ui);
                    });
            },
        );

        ui.add_space(gap);

        // --- RIGHT COLUMN: MARKET & POSITIONS & NEWS ---
        ui.allocate_ui_with_layout(
            egui::vec2(right_panel_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.label(
                    egui::RichText::new(agent.i18n.t("market_and_positions"))
                        .size(12.0)
                        .strong()
                        .color(egui::Color32::from_gray(160)),
                );
                ui.add_space(10.0);

                egui::ScrollArea::vertical()
                    .id_salt("market_list_scroll")
                    .max_height(available_height * 0.35) // Limit height to make room for news/activity
                    .show(ui, |ui| {
                        let mut symbols: Vec<_> = agent.market_data.keys().cloned().collect();
                        symbols.sort();

                        if let Ok(pf) = agent.portfolio.try_read() {
                            for symbol in symbols {
                                let pos = pf.positions.get(&symbol);
                                let is_selected =
                                    agent.selected_chart_tab.as_ref() == Some(&symbol);

                                if render_symbol_card(ui, agent, &symbol, pos, is_selected)
                                    .clicked()
                                {
                                    agent.selected_chart_tab = Some(symbol.clone());
                                }
                                ui.add_space(8.0);
                            }
                        }
                    });

                ui.add_space(15.0);

                // --- NEWS FEED SECTION ---
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📰").size(14.0));
                    ui.label(
                        egui::RichText::new("MARKET NEWS")
                            .size(12.0)
                            .strong()
                            .color(egui::Color32::from_gray(160)),
                    );
                });
                ui.add_space(8.0);

                render_news_feed(ui, &agent.news_events);

                ui.add_space(15.0);
                ui.label(
                    egui::RichText::new(agent.i18n.t("section_recent_activity"))
                        .size(12.0)
                        .strong()
                        .color(egui::Color32::from_gray(160)),
                );
                ui.add_space(10.0);

                render_activity_feed(ui, &agent.activity_feed, &agent.i18n);
            },
        );
    });
}

// --- Helpers ---

fn render_start_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(22, 27, 34))
        .corner_radius(10)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_min_height(100.0);
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .size(12.0)
                    .color(egui::Color32::from_gray(140))
                    .strong(),
            );
            ui.add_space(8.0);
            add_contents(ui);
        });
}

fn render_symbol_card(
    ui: &mut egui::Ui,
    agent: &UserAgent,
    symbol: &str,
    pos: Option<&crate::domain::trading::portfolio::Position>,
    is_selected: bool,
) -> egui::Response {
    let current_price = agent
        .strategy_info
        .get(symbol)
        .map(|i| i.current_price)
        .unwrap_or(
            pos.map(|p| p.average_price)
                .unwrap_or(rust_decimal::Decimal::ZERO),
        );

    let frame = if is_selected {
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(28, 33, 40))
            .corner_radius(8)
            .stroke(egui::Stroke::new(
                1.5,
                egui::Color32::from_rgb(41, 121, 255),
            )) // Blue Active Stroke
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 10,
                spread: 0,
                color: egui::Color32::from_rgba_premultiplied(41, 121, 255, 25), // Blue Glow
            })
            .inner_margin(12)
    } else {
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(28, 33, 40))
            .corner_radius(8)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 54, 61)))
            .inner_margin(12)
    };

    let response = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Header Row: Symbol + P&L or Trend
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(symbol)
                        .size(14.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(pos) = pos {
                        let pnl =
                            (pos.quantity * current_price) - (pos.quantity * pos.average_price);
                        let is_profit = pnl >= rust_decimal::Decimal::ZERO;
                        let pnl_color = if is_profit {
                            egui::Color32::from_rgb(0, 230, 118)
                        } else {
                            egui::Color32::from_rgb(255, 23, 68)
                        };

                        egui::Frame::NONE
                            .fill(pnl_color.linear_multiply(0.15))
                            .corner_radius(12)
                            .inner_margin(egui::Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(agent.i18n.tf(
                                        "pnl_amount_format",
                                        &[
                                            (
                                                "amount",
                                                &format!(
                                                    "{:.2}",
                                                    pnl.to_f64().unwrap_or(0.0).abs()
                                                ),
                                            ),
                                            ("sign", if is_profit { "+" } else { "-" }),
                                        ],
                                    ))
                                    .size(11.0)
                                    .strong()
                                    .color(pnl_color),
                                );
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
                        ui.label(
                            egui::RichText::new(agent.i18n.t("header_quantity"))
                                .size(10.0)
                                .color(egui::Color32::from_gray(120)),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:.4}", pos.quantity))
                                .size(11.0)
                                .color(egui::Color32::from_gray(200)),
                        );
                    });
                    cols[1].vertical(|ui| {
                        ui.label(
                            egui::RichText::new(agent.i18n.t("header_average"))
                                .size(10.0)
                                .color(egui::Color32::from_gray(120)),
                        );
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "currency_format",
                                &[("amount", &format!("{:.2}", pos.average_price))],
                            ))
                            .size(11.0)
                            .color(egui::Color32::from_gray(200)),
                        );
                    });
                    cols[2].vertical(|ui| {
                        ui.label(
                            egui::RichText::new(agent.i18n.t("header_current"))
                                .size(10.0)
                                .color(egui::Color32::from_gray(120)),
                        );
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "currency_format",
                                &[("amount", &format!("{:.2}", current_price))],
                            ))
                            .size(11.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                        );
                    });
                });
            } else {
                // Watchlist Info (Single Row)
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(agent.i18n.t("header_current"))
                            .size(10.0)
                            .color(egui::Color32::from_gray(120)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "currency_format",
                                &[("amount", &format!("{:.2}", current_price))],
                            ))
                            .size(12.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                        );
                    });
                });
            }
        })
        .response;

    ui.interact(response.rect, response.id, egui::Sense::click())
}

// function moved to activity_feed

// functions moved to chart_panel.rs, news_feed.rs, activity_feed.rs (logs), analytics_view.rs
