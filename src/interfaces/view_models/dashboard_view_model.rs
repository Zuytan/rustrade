use crate::application::agents::user_agent::UserAgent;
use eframe::egui;
use rust_decimal::Decimal;

pub struct DashboardMetrics {
    pub total_value: Decimal,
    pub pnl_value: Decimal,
    pub pnl_pct: Decimal,
    pub pnl_color: egui::Color32,
    pub pnl_sign: &'static str,
    pub pnl_arrow: &'static str,
    pub position_count: usize,
    pub market_value: Decimal,
}

pub struct WinRateMetrics {
    pub rate: Decimal,
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
        let total_value = agent.calculate_total_value();

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
                let current_equity = pf.cash + mv;

                // Calculate true daily P&L using starting_cash tracked by RiskManager
                let (pnl, pnl_pct) = if pf.starting_cash > Decimal::ZERO {
                    let p = current_equity - pf.starting_cash;
                    let pct = p / pf.starting_cash * Decimal::from(100);
                    (p, pct)
                } else {
                    // Fallback if starting_cash isn't loaded yet (e.g. startup phase)
                    let p = mv - cost_basis;
                    let pct = if cost_basis > Decimal::ZERO {
                        p / cost_basis * Decimal::from(100)
                    } else {
                        Decimal::ZERO
                    };
                    (p, pct)
                };

                (pnl, pnl_pct, pf.positions.len(), mv)
            }
            Err(_) => (Decimal::ZERO, Decimal::ZERO, 0, Decimal::ZERO),
        };

        let is_positive = pnl_value >= Decimal::ZERO;
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
        let risk_score = agent.settings_panel.risk_score;
        let (label_key, color) = match risk_score {
            1..=3 => ("risk_low", egui::Color32::from_rgb(0, 230, 118)),
            4..=7 => ("risk_medium", egui::Color32::from_rgb(255, 212, 59)),
            _ => ("risk_high", egui::Color32::from_rgb(255, 23, 68)),
        };

        RiskMetrics {
            score: risk_score,
            label_key,
            color,
        }
    }

    pub fn get_sentiment_metrics(agent: &UserAgent) -> SentimentMetrics {
        let has_feeds = !agent.settings_panel.rss_urls.is_empty();

        if agent.symbol_sentiments.is_empty() {
            let (title, is_loading) = if has_feeds {
                (agent.i18n.t("waiting_data").to_string(), true)
            } else {
                (agent.i18n.t("sentiment_unconfigured").to_string(), false)
            };

            return SentimentMetrics {
                title,
                value: 50,
                color: egui::Color32::from_gray(120),
                is_loading,
            };
        }

        // Collect sentiment values for symbols we hold in portfolio,
        // or fall back to all tracked symbols if portfolio is empty.
        let held_symbols: Vec<String> = agent
            .portfolio
            .try_read()
            .map(|pf| pf.positions.keys().cloned().collect())
            .unwrap_or_default();

        let mut relevant_values: Vec<u8> = if held_symbols.is_empty() {
            // No positions — use all known sentiments
            agent.symbol_sentiments.values().map(|s| s.value).collect()
        } else {
            held_symbols
                .iter()
                .filter_map(|sym| agent.symbol_sentiments.get(sym))
                .map(|s| s.value)
                .collect()
        };

        if relevant_values.is_empty() {
            // Fallback to GLOBAL sentiment if present
            if let Some(global) = agent.symbol_sentiments.get("GLOBAL") {
                relevant_values.push(global.value);
            }
        }

        if relevant_values.is_empty() {
            return SentimentMetrics {
                title: agent.i18n.t("sentiment_neutral").to_string(),
                value: 50,
                color: egui::Color32::GRAY,
                is_loading: false,
            };
        }

        let avg: u8 = (relevant_values.iter().map(|v| *v as u16).sum::<u16>()
            / relevant_values.len() as u16) as u8;
        let classification = crate::domain::sentiment::SentimentClassification::from_score(avg);
        let color =
            egui::Color32::from_hex(classification.color_hex()).unwrap_or(egui::Color32::GRAY);

        let label_key = match classification {
            crate::domain::sentiment::SentimentClassification::ExtremeFear => {
                "sentiment_extreme_fear"
            }
            crate::domain::sentiment::SentimentClassification::Fear => "sentiment_fear",
            crate::domain::sentiment::SentimentClassification::Neutral => "sentiment_neutral",
            crate::domain::sentiment::SentimentClassification::Greed => "sentiment_greed",
            crate::domain::sentiment::SentimentClassification::ExtremeGreed => {
                "sentiment_extreme_greed"
            }
        };
        let title = agent.i18n.t(label_key).to_string();

        SentimentMetrics {
            title,
            value: avg,
            color,
            is_loading: false,
        }
    }
}
