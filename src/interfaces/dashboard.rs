use crate::application::agents::user_agent::UserAgent;
use crate::interfaces::components::{
    card::Card,
    charts::render_donut_chart,
    metrics::{render_metric_card, render_status_pill},
};
use crate::interfaces::dashboard_components::{
    activity_feed::render_activity_feed, chart_panel::render_chart_panel,
    news_feed::render_news_feed,
};
use crate::interfaces::design_system::DesignSystem;
use crate::interfaces::view_models::dashboard_view_model::DashboardViewModel;

use eframe::egui;
use rust_decimal::prelude::ToPrimitive;

/// Renders the main Dashboard content
pub fn render_dashboard(ui: &mut egui::Ui, agent: &mut UserAgent) {
    // --- Data Prep (MVVM) ---
    let metrics = DashboardViewModel::get_metrics(agent);
    let win_rate_metrics = DashboardViewModel::get_win_rate(agent);
    let risk_metrics = DashboardViewModel::get_risk_metrics(agent);
    let sentiment_metrics = DashboardViewModel::get_sentiment_metrics(agent);

    // ---------------------------------------------------------
    // 1. TOP HEADER (Total Value + System Status)
    // ---------------------------------------------------------
    ui.add_space(DesignSystem::SPACING_SMALL);
    ui.horizontal(|ui| {
        // Left: Total Value
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new(agent.i18n.tf(
                        "total_value_format",
                        &[("amount", &format!("{:.2}", metrics.total_value))],
                    ))
                    .size(28.0)
                    .strong()
                    .color(DesignSystem::TEXT_PRIMARY),
                );

                ui.add_space(DesignSystem::SPACING_SMALL);

                // Small P&L Pill
                render_status_pill(
                    ui,
                    &agent.i18n.tf(
                        "pnl_pill_format",
                        &[
                            ("amount", &format!("{:.2}", metrics.pnl_value.abs())),
                            ("percent", &format!("{:.2}", metrics.pnl_pct)),
                            ("sign", metrics.pnl_sign),
                        ],
                    ),
                    metrics.pnl_color,
                );
            });
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // System Status
            // We can use a small card or just a group for status
            ui.group(|ui| {
                ui.set_style(ui.style().clone()); // Reset style if needed
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("●")
                            .size(10.0)
                            .color(DesignSystem::SUCCESS),
                    );
                    ui.label(
                        egui::RichText::new(
                            agent
                                .i18n
                                .tf("status_label", &[("status", agent.i18n.t("status_active"))]),
                        )
                        .size(12.0)
                        .color(DesignSystem::TEXT_SECONDARY),
                    );
                    ui.add_space(DesignSystem::SPACING_SMALL);
                    ui.label(
                        egui::RichText::new(
                            agent
                                .i18n
                                .tf("latency_label", &[("ms", &agent.latency_ms.to_string())]),
                        )
                        .size(12.0)
                        .color(DesignSystem::TEXT_MUTED),
                    );
                });
            });
        });
    });

    ui.add_space(DesignSystem::SPACING_LARGE);

    // ---------------------------------------------------------
    // 2. METRICS CARDS (5 Columns)
    // ---------------------------------------------------------
    ui.columns(5, |columns| {
        // Card 1: DAILY P&L
        columns[0].push_id("card_daily_pnl", |ui| {
            render_metric_card(
                ui,
                agent.i18n.t("metric_daily_pnl"),
                &agent.i18n.tf(
                    "pnl_value_format",
                    &[
                        ("amount", &format!("{:.2}", metrics.pnl_value.abs())),
                        ("sign", metrics.pnl_sign),
                    ],
                ),
                metrics.pnl_color,
                Some(agent.i18n.t("last_24h")), // Context
                Some(metrics.pnl_arrow),        // Icon
                true,                           // Active styling
            );
        });

        // Card 2: WIN RATE
        columns[1].push_id("card_win_rate", |ui| {
            Card::new()
                .title(agent.i18n.t("metric_win_rate"))
                .min_height(100.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(agent.i18n.tf(
                                    "percent_format",
                                    &[("value", &format!("{:.1}", win_rate_metrics.rate))],
                                ))
                                .size(28.0)
                                .strong()
                                .color(DesignSystem::BORDER_FOCUS),
                            );
                            ui.label(
                                egui::RichText::new(agent.i18n.tf(
                                    "trades_count_format",
                                    &[
                                        ("winning", &win_rate_metrics.winning_trades.to_string()),
                                        ("total", &win_rate_metrics.total_trades.to_string()),
                                    ],
                                ))
                                .size(11.0)
                                .color(DesignSystem::TEXT_MUTED),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            render_donut_chart(
                                ui,
                                win_rate_metrics.rate as f32,
                                DesignSystem::BORDER_FOCUS,
                                40.0,
                            );
                        });
                    });
                });
        });

        // Card 3: OPEN POSITIONS
        columns[2].push_id("card_open_pos", |ui| {
            render_metric_card(
                ui,
                agent.i18n.t("metric_open_positions"),
                &format!("{}", metrics.position_count),
                DesignSystem::TEXT_PRIMARY,
                Some(&agent.i18n.tf(
                    "total_volume_format",
                    &[("amount", &format!("{:.0}", metrics.market_value))],
                )),
                Some("🪙"),
                false,
            );
        });

        // Card 4: RISK SCORE
        columns[3].push_id("card_risk", |ui| {
            render_metric_card(
                ui,
                agent.i18n.t("metric_risk_score"),
                agent.i18n.t(risk_metrics.label_key),
                risk_metrics.color,
                Some(&agent.i18n.tf(
                    "risk_score_label_short",
                    &[("score", &risk_metrics.score.to_string())],
                )),
                Some("🛡"),
                false,
            );
        });

        // Card 5: MARKET MOOD
        columns[4].push_id("card_market_mood", |ui| {
            Card::new()
                .title("MARKET MOOD")
                .min_height(100.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            if !sentiment_metrics.is_loading {
                                ui.label(
                                    egui::RichText::new(&sentiment_metrics.title)
                                        .size(22.0)
                                        .strong()
                                        .color(sentiment_metrics.color),
                                );

                                // Progress Bar
                                let (rect, _resp) = ui.allocate_at_least(
                                    egui::vec2(100.0, 6.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, 3.0, DesignSystem::BORDER_SUBTLE);

                                let progress_width =
                                    100.0 * (sentiment_metrics.value as f32 / 100.0);
                                let progress_rect = egui::Rect::from_min_size(
                                    rect.min,
                                    egui::vec2(progress_width, 6.0),
                                );
                                ui.painter().rect_filled(
                                    progress_rect,
                                    3.0,
                                    sentiment_metrics.color,
                                );

                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Index: {}",
                                        sentiment_metrics.value
                                    ))
                                    .size(11.0)
                                    .color(DesignSystem::TEXT_MUTED),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(&sentiment_metrics.title)
                                        .size(22.0)
                                        .strong()
                                        .color(sentiment_metrics.color),
                                );
                                ui.label(
                                    egui::RichText::new("Waiting for data")
                                        .size(11.0)
                                        .color(DesignSystem::TEXT_MUTED),
                                );
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new("🌡")
                                    .size(24.0)
                                    .color(DesignSystem::TEXT_MUTED),
                            );
                        });
                    });
                });
        });
    });

    ui.add_space(DesignSystem::SPACING_LARGE);

    // ---------------------------------------------------------
    // 3. MAIN SPLIT VIEW (Charts vs Live Positions)
    // ---------------------------------------------------------
    let available_height = ui.available_height() - 30.0;
    let total_width = ui.available_width();
    let gap = DesignSystem::SPACING_MEDIUM;

    // Adjust Proportions (Chart ~65%, Positions ~35%)
    let chart_width = (total_width * 0.65 - gap).max(200.0);
    let right_panel_width = total_width - chart_width - gap;

    ui.horizontal(|ui| {
        // --- LEFT COLUMN: CHART ---
        ui.allocate_ui_with_layout(
            egui::vec2(chart_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                Card::new().show(ui, |ui| {
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
                        .color(DesignSystem::TEXT_SECONDARY),
                );
                ui.add_space(DesignSystem::SPACING_SMALL);

                egui::ScrollArea::vertical()
                    .id_salt("market_list_scroll")
                    .max_height(available_height * 0.35)
                    .show(ui, |ui| {
                        let mut symbol_set: std::collections::HashSet<String> =
                            agent.market_data.keys().cloned().collect();

                        if let Ok(pf) = agent.portfolio.try_read() {
                            for key in pf.positions.keys() {
                                symbol_set.insert(key.clone());
                            }
                        }

                        let mut symbols: Vec<_> = symbol_set.into_iter().collect();
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
                                ui.add_space(DesignSystem::SPACING_SMALL);
                            }
                        }
                    });

                ui.add_space(DesignSystem::SPACING_MEDIUM);

                // --- NEWS FEED SECTION ---
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📰").size(14.0));
                    ui.label(
                        egui::RichText::new("MARKET NEWS")
                            .size(12.0)
                            .strong()
                            .color(DesignSystem::TEXT_SECONDARY),
                    );
                });
                ui.add_space(DesignSystem::SPACING_SMALL);

                render_news_feed(ui, &agent.news_events);

                ui.add_space(DesignSystem::SPACING_MEDIUM);
                ui.label(
                    egui::RichText::new(agent.i18n.t("section_recent_activity"))
                        .size(12.0)
                        .strong()
                        .color(DesignSystem::TEXT_SECONDARY),
                );
                ui.add_space(DesignSystem::SPACING_SMALL);

                render_activity_feed(ui, &agent.activity_feed, &agent.i18n);
            },
        );
    });
}

// --- Helpers ---

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

    // Using Card logic manually here because we need interaction on the whole card
    // and custom "selected" styling

    let border_color = if is_selected {
        DesignSystem::ACCENT_PRIMARY
    } else {
        DesignSystem::BORDER_SUBTLE
    };
    let bg_color = if is_selected {
        DesignSystem::BG_CARD_HOVER
    } else {
        DesignSystem::BG_CARD
    };
    let border_width = if is_selected { 1.5 } else { 1.0 };

    let mut frame = egui::Frame::NONE
        .fill(bg_color)
        .corner_radius(DesignSystem::ROUNDING_MEDIUM)
        .stroke(egui::Stroke::new(border_width, border_color))
        .inner_margin(DesignSystem::SPACING_MEDIUM);

    if is_selected {
        frame = frame.shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 0,
            color: DesignSystem::ACCENT_PRIMARY.linear_multiply(0.15),
        });
    }

    let response = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Header Row: Symbol + P&L or Trend
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(symbol)
                        .size(14.0)
                        .strong()
                        .color(DesignSystem::TEXT_PRIMARY),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(pos) = pos {
                        let pnl =
                            (pos.quantity * current_price) - (pos.quantity * pos.average_price);
                        let is_profit = pnl >= rust_decimal::Decimal::ZERO;
                        let pnl_color = if is_profit {
                            DesignSystem::SUCCESS
                        } else {
                            DesignSystem::DANGER
                        };

                        render_status_pill(
                            ui,
                            &agent.i18n.tf(
                                "pnl_amount_format",
                                &[
                                    (
                                        "amount",
                                        &format!("{:.2}", pnl.to_f64().unwrap_or(0.0).abs()),
                                    ),
                                    ("sign", if is_profit { "+" } else { "-" }),
                                ],
                            ),
                            pnl_color,
                        );
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
                                .color(DesignSystem::TEXT_MUTED),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:.4}", pos.quantity))
                                .size(11.0)
                                .color(DesignSystem::TEXT_SECONDARY),
                        );
                    });
                    cols[1].vertical(|ui| {
                        ui.label(
                            egui::RichText::new(agent.i18n.t("header_average"))
                                .size(10.0)
                                .color(DesignSystem::TEXT_MUTED),
                        );
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "currency_format",
                                &[("amount", &format!("{:.2}", pos.average_price))],
                            ))
                            .size(11.0)
                            .color(DesignSystem::TEXT_SECONDARY),
                        );
                    });
                    cols[2].vertical(|ui| {
                        ui.label(
                            egui::RichText::new(agent.i18n.t("header_current"))
                                .size(10.0)
                                .color(DesignSystem::TEXT_MUTED),
                        );
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "currency_format",
                                &[("amount", &format!("{:.2}", current_price))],
                            ))
                            .size(11.0)
                            .strong()
                            .color(DesignSystem::TEXT_PRIMARY),
                        );
                    });
                });
            } else {
                // Watchlist Info (Single Row)
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(agent.i18n.t("header_current"))
                            .size(10.0)
                            .color(DesignSystem::TEXT_MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(agent.i18n.tf(
                                "currency_format",
                                &[("amount", &format!("{:.2}", current_price))],
                            ))
                            .size(12.0)
                            .strong()
                            .color(DesignSystem::TEXT_PRIMARY),
                        );
                    });
                });
            }
        })
        .response;

    ui.interact(response.rect, response.id, egui::Sense::click())
}
