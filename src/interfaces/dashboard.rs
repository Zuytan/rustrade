use crate::application::agents::user_agent::UserAgent;
use crate::interfaces::components::{
    card::Card,
    metrics::{render_metric_card, render_status_pill},
};
use crate::interfaces::dashboard_components::{
    activity_feed::render_activity_feed, chart_panel::render_chart_panel,
    news_feed::render_news_feed, symbol_card::render_symbol_card,
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
                .min_height(110.0)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        // Value + Icon row
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{:.1}%", win_rate_metrics.rate))
                                    .size(28.0)
                                    .strong()
                                    .color(DesignSystem::TEXT_PRIMARY),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new("🎯")
                                            .size(24.0)
                                            .color(DesignSystem::TEXT_MUTED),
                                    );
                                },
                            );
                        });

                        ui.add_space(DesignSystem::SPACING_SMALL);

                        // Custom thin progress bar
                        let (rect, _) = ui.allocate_at_least(
                            egui::vec2(ui.available_width(), 6.0),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .rect_filled(rect, 3.0, DesignSystem::BORDER_SUBTLE);
                        let progress_width =
                            rect.width() * (win_rate_metrics.rate.to_f32().unwrap_or(0.0) / 100.0);
                        let progress_rect =
                            egui::Rect::from_min_size(rect.min, egui::vec2(progress_width, 6.0));
                        ui.painter()
                            .rect_filled(progress_rect, 3.0, DesignSystem::ACCENT_PRIMARY);

                        ui.add_space(DesignSystem::SPACING_SMALL);

                        // Subtitle
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

        // Card 5: PORTFOLIO MOOD
        columns[4].push_id("card_market_mood", |ui| {
            Card::new()
                .title("PORTFOLIO MOOD")
                .min_height(110.0)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        if !sentiment_metrics.is_loading {
                            // Value + Icon row
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&sentiment_metrics.title)
                                        .size(28.0)
                                        .strong()
                                        .color(sentiment_metrics.color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new("🌡")
                                                .size(24.0)
                                                .color(DesignSystem::TEXT_MUTED),
                                        );
                                    },
                                );
                            });

                            ui.add_space(DesignSystem::SPACING_SMALL);

                            // Custom thin progress bar
                            let (rect, _) = ui.allocate_at_least(
                                egui::vec2(ui.available_width(), 6.0),
                                egui::Sense::hover(),
                            );
                            ui.painter()
                                .rect_filled(rect, 3.0, DesignSystem::BORDER_SUBTLE);
                            let progress_width =
                                rect.width() * (sentiment_metrics.value as f32 / 100.0);
                            let progress_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(progress_width, 6.0),
                            );
                            ui.painter()
                                .rect_filled(progress_rect, 3.0, sentiment_metrics.color);

                            ui.add_space(DesignSystem::SPACING_SMALL);

                            ui.label(
                                egui::RichText::new(format!("Index: {}", sentiment_metrics.value))
                                    .size(11.0)
                                    .color(DesignSystem::TEXT_MUTED),
                            );
                        } else {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&sentiment_metrics.title)
                                        .size(28.0)
                                        .strong()
                                        .color(sentiment_metrics.color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new("🌡")
                                                .size(24.0)
                                                .color(DesignSystem::TEXT_MUTED),
                                        );
                                    },
                                );
                            });
                            ui.add_space(DesignSystem::SPACING_SMALL);
                            ui.label(
                                egui::RichText::new("Waiting for data")
                                    .size(11.0)
                                    .color(DesignSystem::TEXT_MUTED),
                            );
                        }
                    });
                });
        });
    });

    ui.add_space(DesignSystem::SPACING_MEDIUM);

    // --- Performance Ratios Banner ---
    let perf_metrics = agent.get_performance_metrics();
    ui.horizontal(|ui| {
        ui.add_space(DesignSystem::SPACING_SMALL);
        ui.label(
            egui::RichText::new("Performance Ratios:")
                .size(11.0)
                .color(DesignSystem::TEXT_MUTED)
                .strong(),
        );
        ui.add_space(DesignSystem::SPACING_SMALL);

        // Sharpe Ratio
        ui.label(
            egui::RichText::new("Sharpe:")
                .size(11.0)
                .color(DesignSystem::TEXT_SECONDARY),
        );
        let sharpe_color = if perf_metrics.sharpe_ratio >= 2.0 {
            DesignSystem::SUCCESS
        } else if perf_metrics.sharpe_ratio >= 1.0 {
            DesignSystem::TEXT_PRIMARY
        } else {
            DesignSystem::TEXT_MUTED
        };
        ui.label(
            egui::RichText::new(format!("{:.2}", perf_metrics.sharpe_ratio))
                .size(11.0)
                .strong()
                .color(sharpe_color),
        );

        ui.add_space(DesignSystem::SPACING_MEDIUM);
        ui.label(
            egui::RichText::new("|")
                .size(11.0)
                .color(DesignSystem::BORDER_SUBTLE),
        );
        ui.add_space(DesignSystem::SPACING_MEDIUM);

        // Sortino Ratio
        ui.label(
            egui::RichText::new("Sortino:")
                .size(11.0)
                .color(DesignSystem::TEXT_SECONDARY),
        );
        let sortino_color = if perf_metrics.sortino_ratio >= 2.0 {
            DesignSystem::SUCCESS
        } else if perf_metrics.sortino_ratio >= 1.0 {
            DesignSystem::TEXT_PRIMARY
        } else {
            DesignSystem::TEXT_MUTED
        };
        ui.label(
            egui::RichText::new(format!("{:.2}", perf_metrics.sortino_ratio))
                .size(11.0)
                .strong()
                .color(sortino_color),
        );

        ui.add_space(DesignSystem::SPACING_MEDIUM);
        ui.label(
            egui::RichText::new("|")
                .size(11.0)
                .color(DesignSystem::BORDER_SUBTLE),
        );
        ui.add_space(DesignSystem::SPACING_MEDIUM);

        // Profit Factor
        ui.label(
            egui::RichText::new("Profit Factor:")
                .size(11.0)
                .color(DesignSystem::TEXT_SECONDARY),
        );
        let pf_color = if perf_metrics.profit_factor >= 1.5 {
            DesignSystem::SUCCESS
        } else if perf_metrics.profit_factor >= 1.0 {
            DesignSystem::TEXT_PRIMARY
        } else {
            DesignSystem::DANGER
        };
        ui.label(
            egui::RichText::new(format!("{:.2}", perf_metrics.profit_factor))
                .size(11.0)
                .strong()
                .color(pf_color),
        );
    });

    ui.add_space(DesignSystem::SPACING_MEDIUM);

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

        // --- RIGHT COLUMN: MARKET & POSITIONS & NEWS & ACTIVITY ---
        ui.allocate_ui_with_layout(
            egui::vec2(right_panel_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                let card1_height = (available_height * 0.45 - gap / 2.0).max(150.0);
                let card2_height = (available_height * 0.55 - gap / 2.0).max(150.0);

                // Card 1: Market & Positions
                Card::new()
                    .title(agent.i18n.t("market_and_positions"))
                    .min_height(card1_height)
                    .show(ui, |ui| {
                        let scroll_height = card1_height - 50.0;
                        egui::ScrollArea::vertical()
                            .id_salt("market_list_scroll")
                            .max_height(scroll_height)
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
                    });

                ui.add_space(gap);

                // Card 2: News & Activity Feed (Tabbed)
                Card::new().min_height(card2_height).show(ui, |ui| {
                    let scroll_height = card2_height - 60.0;

                    ui.horizontal(|ui| {
                        let is_news = agent.right_panel_tab
                            == crate::application::agents::user_agent::RightPanelTab::News;
                        let is_activity = agent.right_panel_tab
                            == crate::application::agents::user_agent::RightPanelTab::Activity;

                        let news_text = if is_news {
                            egui::RichText::new("📰 News")
                                .strong()
                                .color(DesignSystem::ACCENT_PRIMARY)
                        } else {
                            egui::RichText::new("📰 News").color(DesignSystem::TEXT_SECONDARY)
                        };
                        if ui.selectable_label(is_news, news_text).clicked() {
                            agent.right_panel_tab =
                                crate::application::agents::user_agent::RightPanelTab::News;
                        }

                        ui.add_space(16.0);

                        let activity_text = if is_activity {
                            egui::RichText::new("⚡ Activity")
                                .strong()
                                .color(DesignSystem::ACCENT_PRIMARY)
                        } else {
                            egui::RichText::new("⚡ Activity").color(DesignSystem::TEXT_SECONDARY)
                        };
                        if ui.selectable_label(is_activity, activity_text).clicked() {
                            agent.right_panel_tab =
                                crate::application::agents::user_agent::RightPanelTab::Activity;
                        }
                    });

                    ui.add_space(DesignSystem::SPACING_SMALL);
                    ui.separator();
                    ui.add_space(DesignSystem::SPACING_SMALL);

                    match agent.right_panel_tab {
                        crate::application::agents::user_agent::RightPanelTab::News => {
                            render_news_feed(ui, &agent.news_events, scroll_height);
                        }
                        crate::application::agents::user_agent::RightPanelTab::Activity => {
                            render_activity_feed(
                                ui,
                                &agent.activity_feed,
                                &agent.i18n,
                                scroll_height,
                            );
                        }
                    }
                });
            },
        );
    });
}

// --- Helpers ---
// The render_symbol_card helper has been moved to dashboard_components::symbol_card
