use crate::application::agents::user_agent::UserAgent;
use crate::interfaces::components::metrics::render_status_pill;
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;
use rust_decimal::prelude::ToPrimitive;

pub fn render_symbol_card(
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
