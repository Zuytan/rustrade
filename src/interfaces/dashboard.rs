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

    // --- Dynamic Sizing (based on window height percentage) ---
    let total_width = ui.available_width();
    let total_height = ui.available_height();
    let metrics_card_height = (total_height * 0.15).clamp(55.0, 150.0);
    // Calculate the remaining height dynamically to prevent vertical overflow.
    // 110.0 accounts for top header (~35.0), perf bar (~35.0), and spacing/margins.
    let split_view_height = (total_height - metrics_card_height - 110.0).max(100.0);

    // ---------------------------------------------------------
    // 1. TOP HEADER (Total Value + System Status)
    // ---------------------------------------------------------
    ui.add_space(DesignSystem::SPACING_SMALL);
    ui.horizontal(|ui| {
        let viewport_w = ui.ctx().viewport_rect().width();
        let header_font_size = if viewport_w < 700.0 { 18.0 } else { 28.0 };

        // Left: Total Value
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading(
                    egui::RichText::new(agent.i18n.tf(
                        "total_value_format",
                        &[("amount", &format!("{:.2}", metrics.total_value))],
                    ))
                    .size(header_font_size)
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

        // Hide system status on small screens to prevent layout overflow
        if viewport_w >= 600.0 {
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

                        let status_text = if viewport_w < 800.0 {
                            agent
                                .i18n
                                .t("status_active")
                                .replace(" - Moteur HFT en cours", "")
                                .replace(" - HFT Engine Running", "")
                        } else {
                            agent
                                .i18n
                                .tf("status_label", &[("status", agent.i18n.t("status_active"))])
                        };

                        ui.label(
                            egui::RichText::new(status_text)
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
        }
    });

    ui.add_space(DesignSystem::SPACING_SMALL);

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
                metrics_card_height,
            );
        });

        // Card 2: WIN RATE
        columns[1].push_id("card_win_rate", |ui| {
            let rate_f32 = win_rate_metrics.rate.to_f32().unwrap_or(0.0);
            let win_rate_color = if win_rate_metrics.total_trades == 0 {
                DesignSystem::TEXT_MUTED
            } else if rate_f32 >= 60.0 {
                DesignSystem::SUCCESS
            } else if rate_f32 >= 40.0 {
                DesignSystem::WARNING
            } else {
                DesignSystem::DANGER
            };

            Card::new()
                .title(agent.i18n.t("metric_win_rate"))
                .min_height(metrics_card_height)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        let available_h = ui.available_height();
                        let est_col_w = (total_width - 32.0) / 5.0;
                        let show_details = available_h > 45.0 && est_col_w > 100.0;

                        let font_size = if est_col_w < 90.0 {
                            12.0
                        } else if est_col_w < 120.0 {
                            16.0
                        } else if est_col_w < 150.0 {
                            22.0
                        } else {
                            28.0
                        };

                        // Value + Icon row
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{:.1}%", win_rate_metrics.rate))
                                        .size(font_size)
                                        .strong()
                                        .color(win_rate_color),
                                )
                                .wrap(),
                            );
                            if show_details {
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
                            }
                        });

                        if show_details {
                            ui.add_space(DesignSystem::SPACING_SMALL);

                            // Custom thin progress bar
                            let (rect, _) = ui.allocate_at_least(
                                egui::vec2(ui.available_width(), 6.0),
                                egui::Sense::hover(),
                            );
                            ui.painter()
                                .rect_filled(rect, 3.0, DesignSystem::BORDER_SUBTLE);
                            let progress_width = rect.width() * (rate_f32 / 100.0);
                            let progress_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(progress_width, 6.0),
                            );
                            ui.painter().rect_filled(progress_rect, 3.0, win_rate_color);

                            ui.add_space(DesignSystem::SPACING_SMALL);

                            // Subtitle
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(agent.i18n.tf(
                                        "trades_count_format",
                                        &[
                                            (
                                                "winning",
                                                &win_rate_metrics.winning_trades.to_string(),
                                            ),
                                            ("total", &win_rate_metrics.total_trades.to_string()),
                                        ],
                                    ))
                                    .size(11.0)
                                    .color(DesignSystem::TEXT_MUTED),
                                )
                                .wrap(),
                            );
                        }
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
                metrics_card_height,
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
                metrics_card_height,
            );
        });

        // Card 5: PORTFOLIO MOOD
        columns[4].push_id("card_market_mood", |ui| {
            Card::new()
                .title(agent.i18n.t("portfolio_mood"))
                .min_height(metrics_card_height)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        let available_h = ui.available_height();
                        let est_col_w = (total_width - 32.0) / 5.0;
                        let show_details = available_h > 45.0 && est_col_w > 100.0;

                        if !sentiment_metrics.is_loading {
                            // Value
                            ui.horizontal(|ui| {
                                let font_size = if est_col_w < 100.0 {
                                    11.0
                                } else if est_col_w < 130.0 {
                                    14.0
                                } else if est_col_w < 160.0 {
                                    18.0
                                } else {
                                    22.0
                                };

                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&sentiment_metrics.title)
                                            .size(font_size)
                                            .strong()
                                            .color(sentiment_metrics.color),
                                    )
                                    .wrap(),
                                );
                                if show_details {
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
                                }
                            });

                            if show_details {
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
                                ui.painter().rect_filled(
                                    progress_rect,
                                    3.0,
                                    sentiment_metrics.color,
                                );

                                ui.add_space(DesignSystem::SPACING_SMALL);

                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(agent.i18n.tf(
                                            "sentiment_index",
                                            &[("value", &sentiment_metrics.value.to_string())],
                                        ))
                                        .size(11.0)
                                        .color(DesignSystem::TEXT_MUTED),
                                    )
                                    .wrap(),
                                );
                            }
                        } else {
                            ui.horizontal(|ui| {
                                let font_size = if est_col_w < 100.0 {
                                    11.0
                                } else if est_col_w < 130.0 {
                                    14.0
                                } else if est_col_w < 160.0 {
                                    18.0
                                } else {
                                    22.0
                                };

                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&sentiment_metrics.title)
                                            .size(font_size)
                                            .strong()
                                            .color(sentiment_metrics.color),
                                    )
                                    .wrap(),
                                );
                                if show_details {
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
                                }
                            });
                            if show_details {
                                ui.add_space(DesignSystem::SPACING_SMALL);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(agent.i18n.t("waiting_data"))
                                            .size(11.0)
                                            .color(DesignSystem::TEXT_MUTED),
                                    )
                                    .wrap(),
                                );
                            }
                        }
                    });
                });
        });
    });

    ui.add_space(DesignSystem::SPACING_SMALL);

    // --- Performance Ratios Sleek Bar ---
    let perf_metrics = agent.get_performance_metrics();
    egui::Frame::NONE
        .fill(DesignSystem::BG_CARD)
        .corner_radius(DesignSystem::ROUNDING_SMALL)
        .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
        .inner_margin(egui::Margin::symmetric(16, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "📊 {}",
                        agent.i18n.t("perf_ratios_title").replace(":", "").trim()
                    ))
                    .size(11.0)
                    .color(DesignSystem::TEXT_SECONDARY)
                    .strong(),
                );

                ui.add_space(DesignSystem::SPACING_MEDIUM);
                ui.separator();
                ui.add_space(DesignSystem::SPACING_MEDIUM);

                // Sharpe Ratio
                ui.label(
                    egui::RichText::new(agent.i18n.t("sharpe_label").replace(":", "").trim())
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
                ui.separator();
                ui.add_space(DesignSystem::SPACING_MEDIUM);

                // Sortino Ratio
                ui.label(
                    egui::RichText::new(agent.i18n.t("sortino_label").replace(":", "").trim())
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
                ui.separator();
                ui.add_space(DesignSystem::SPACING_MEDIUM);

                // Profit Factor
                ui.label(
                    egui::RichText::new(
                        agent.i18n.t("profit_factor_label").replace(":", "").trim(),
                    )
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
        });

    ui.add_space(DesignSystem::SPACING_SMALL);

    // ---------------------------------------------------------
    // 3. MAIN SPLIT VIEW (Charts vs Live Positions)
    // ---------------------------------------------------------
    let available_height = split_view_height;
    let gap = DesignSystem::SPACING_MEDIUM;

    // Adjust Proportions (Chart ~65%, Positions ~35%), ensuring right panel doesn't squeeze too small or overflow
    let right_min_w = 160.0;
    let chart_width = if total_width - right_min_w - gap < 200.0 {
        (total_width - right_min_w - gap).max(100.0)
    } else {
        let w = total_width * 0.65 - gap;
        if total_width - w - gap < right_min_w {
            total_width - right_min_w - gap
        } else {
            w
        }
    };
    let right_panel_width = (total_width - chart_width - gap).max(right_min_w);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
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
                let card1_height = (available_height * 0.45 - gap / 2.0).max(100.0);
                let card2_height = (available_height - card1_height - gap).max(100.0);

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
                            egui::RichText::new(agent.i18n.t("news_tab"))
                                .strong()
                                .color(DesignSystem::ACCENT_PRIMARY)
                        } else {
                            egui::RichText::new(agent.i18n.t("news_tab"))
                                .color(DesignSystem::TEXT_SECONDARY)
                        };
                        if ui.selectable_label(is_news, news_text).clicked() {
                            agent.right_panel_tab =
                                crate::application::agents::user_agent::RightPanelTab::News;
                        }

                        ui.add_space(16.0);

                        let activity_text = if is_activity {
                            egui::RichText::new(agent.i18n.t("activity_tab"))
                                .strong()
                                .color(DesignSystem::ACCENT_PRIMARY)
                        } else {
                            egui::RichText::new(agent.i18n.t("activity_tab"))
                                .color(DesignSystem::TEXT_SECONDARY)
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
                            let has_feeds = !agent.settings_panel.rss_urls.is_empty();
                            render_news_feed(
                                ui,
                                &agent.news_events,
                                &agent.i18n,
                                scroll_height,
                                has_feeds,
                            );
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
