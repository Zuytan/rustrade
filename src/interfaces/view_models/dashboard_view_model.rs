use crate::application::agents::user_agent::UserAgent;
use eframe::egui;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

pub struct DashboardMetrics {
    pub total_value: f64,
    pub pnl_value: f64,
    pub pnl_pct: f64,
    pub pnl_color: egui::Color32,
    pub pnl_sign: &'static str,
    pub pnl_arrow: &'static str,
    pub position_count: usize,
    pub market_value: f64,
}

pub struct WinRateMetrics {
    pub rate: f64,
    pub winning_trades: usize,
    pub total_trades: usize,
}

pub struct RiskMetrics {
    pub score: u8,
    pub label_key: &'static str,
    pub color: egui::Color32,
}

pub struct SentimentMetrics {
    pub title: String,
    pub value: u8,
    pub color: egui::Color32,
    pub is_loading: bool,
}

pub struct DashboardViewModel;

impl DashboardViewModel {
    pub fn get_metrics(agent: &UserAgent) -> DashboardMetrics {
        let total_value = agent.calculate_total_value().to_f64().unwrap_or(0.0);

        let (pnl_value, pnl_pct, position_count, market_value) = match agent.portfolio.try_read() {
            Ok(pf) => {
                let mut cost_basis = Decimal::ZERO;
                let mut mv = Decimal::ZERO;
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
                let pnl_pct = if cost_basis > Decimal::ZERO {
                    (pnl / cost_basis * Decimal::from(100))
                        .to_f64()
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                (
                    pnl.to_f64().unwrap_or(0.0),
                    pnl_pct,
                    pf.positions.len(),
                    mv.to_f64().unwrap_or(0.0),
                )
            }
            Err(_) => (0.0, 0.0, 0, 0.0),
        };

        let is_positive = pnl_value >= 0.0;
        let pnl_color = if is_positive {
            egui::Color32::from_rgb(0, 230, 118) // Neon Green
        } else {
            egui::Color32::from_rgb(255, 23, 68) // Neon Red
        };

        DashboardMetrics {
            total_value,
            pnl_value,
            pnl_pct,
            pnl_color,
            pnl_sign: if is_positive { "+" } else { "" },
            pnl_arrow: if is_positive { "↗" } else { "↘" },
            position_count,
            market_value,
        }
    }

    pub fn get_win_rate(agent: &UserAgent) -> WinRateMetrics {
        WinRateMetrics {
            rate: agent.calculate_win_rate(),
            winning_trades: agent.winning_trades,
            total_trades: agent.total_trades,
        }
    }

    pub fn get_risk_metrics(agent: &UserAgent) -> RiskMetrics {
        let (label_key, color) = match agent.risk_score {
            1..=3 => ("risk_low", egui::Color32::from_rgb(0, 230, 118)),
            4..=7 => ("risk_medium", egui::Color32::from_rgb(255, 212, 59)),
            _ => ("risk_high", egui::Color32::from_rgb(255, 23, 68)),
        };

        RiskMetrics {
            score: agent.risk_score,
            label_key,
            color,
        }
    }

    pub fn get_sentiment_metrics(agent: &UserAgent) -> SentimentMetrics {
        if let Some(sentiment) = &agent.market_sentiment {
            let color = egui::Color32::from_hex(sentiment.classification.color_hex())
                .unwrap_or(egui::Color32::GRAY);

            SentimentMetrics {
                title: sentiment.classification.to_string(),
                value: sentiment.value,
                color,
                is_loading: false,
            }
        } else {
            SentimentMetrics {
                title: "Loading...".to_string(),
                value: 0,
                color: egui::Color32::GRAY,
                is_loading: true,
            }
        }
    }
}
