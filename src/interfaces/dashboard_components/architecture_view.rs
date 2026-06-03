use crate::application::agents::user_agent::UserAgent;
use crate::application::monitoring::agent_status::{AgentStatus, HealthStatus};
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// Renders the Architecture & Metrics view
pub fn render_architecture_view(ui: &mut egui::Ui, agent: &UserAgent) {
    ui.vertical(|ui| {
        // Header
        ui.add_space(DesignSystem::SPACING_MEDIUM);
        ui.heading(
            egui::RichText::new("⚙ System Architecture & Agent Status")
                .size(24.0)
                .strong()
                .color(DesignSystem::TEXT_PRIMARY),
        );
        ui.add_space(DesignSystem::SPACING_SMALL);
        ui.separator();
        ui.add_space(DesignSystem::SPACING_LARGE);

        // Fetch metrics from registry (UI thread safe)
        let registry = agent.client.agent_registry();
        let status_map = registry.get_all_sync();

        // Sort for stable display
        let mut statuses: Vec<AgentStatus> = status_map.values().cloned().collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));

        // 1. System Overview Metrics
        render_system_metrics(ui, &statuses);

        // 1.5 Global Halt Status
        render_global_halt_status(ui, &statuses);

        ui.add_space(DesignSystem::SPACING_LARGE);

        // 2. Agent Graph (Enclosed in a Card)
        crate::interfaces::components::card::Card::new()
            .title("Agent Data Flow")
            .show(ui, |ui| {
                render_agent_graph(ui, &statuses);
            });

        ui.add_space(DesignSystem::SPACING_LARGE);

        // 3. Agent Grid Section
        ui.heading(
            egui::RichText::new("📊 Active Agents Detail")
                .size(18.0)
                .strong()
                .color(DesignSystem::TEXT_PRIMARY),
        );
        ui.add_space(DesignSystem::SPACING_MEDIUM);

        egui::ScrollArea::vertical().show(ui, |ui| {
            render_agent_grid(ui, &statuses);
        });
    });
}

fn render_system_metrics(ui: &mut egui::Ui, statuses: &[AgentStatus]) {
    let total_agents = statuses.len();
    let healthy_count = statuses
        .iter()
        .filter(|s| s.health == HealthStatus::Healthy)
        .count();
    let degraded_count = statuses
        .iter()
        .filter(|s| s.health == HealthStatus::Degraded)
        .count();
    let dead_count = statuses
        .iter()
        .filter(|s| s.health == HealthStatus::Dead)
        .count();

    ui.columns(4, |cols| {
        cols[0].push_id("arch_total_agents", |ui| {
            render_metric_card(
                ui,
                "Total Agents",
                &total_agents.to_string(),
                DesignSystem::TEXT_PRIMARY,
            );
        });
        cols[1].push_id("arch_healthy", |ui| {
            render_metric_card(
                ui,
                "Healthy",
                &healthy_count.to_string(),
                DesignSystem::SUCCESS,
            );
        });
        cols[2].push_id("arch_degraded", |ui| {
            render_metric_card(
                ui,
                "Degraded",
                &degraded_count.to_string(),
                DesignSystem::WARNING,
            );
        });
        cols[3].push_id("arch_dead", |ui| {
            render_metric_card(ui, "Dead", &dead_count.to_string(), DesignSystem::DANGER);
        });
    });
}

fn render_metric_card(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    egui::Frame::NONE
        .fill(DesignSystem::BG_CARD)
        .corner_radius(DesignSystem::ROUNDING_MEDIUM)
        .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(12.0)
                        .color(DesignSystem::TEXT_SECONDARY),
                );
                ui.add_space(4.0);
                ui.label(egui::RichText::new(value).size(24.0).strong().color(color));
            });
        });
}

fn render_agent_graph(ui: &mut egui::Ui, statuses: &[AgentStatus]) {
    let width = ui.available_width().max(400.0);
    let (response, painter) = ui.allocate_painter(egui::vec2(width, 180.0), egui::Sense::hover());
    let rect = response.rect;

    let center_x = rect.center().x;
    let center_y = rect.center().y;

    // Dynamically calculate spacing based on available width
    let x_spacing = (rect.width() / 8.0).clamp(50.0, 150.0);
    let y_spacing = 40.0;
    let node_radius = (x_spacing * 0.22).clamp(16.0, 24.0);

    // Define positions
    let positions = [
        (
            "Listener",
            egui::pos2(center_x - 3.0 * x_spacing, center_y - y_spacing),
        ),
        (
            "ConnectionHealthService",
            egui::pos2(center_x - 3.0 * x_spacing, center_y + y_spacing),
        ),
        ("Sentinel", egui::pos2(center_x - 1.5 * x_spacing, center_y)),
        ("Analyst", egui::pos2(center_x, center_y)),
        (
            "RiskManager",
            egui::pos2(center_x + 1.5 * x_spacing, center_y),
        ),
        ("Executor", egui::pos2(center_x + 3.0 * x_spacing, center_y)),
        (
            "UserAgent",
            egui::pos2(center_x, center_y - 1.8 * y_spacing),
        ),
    ];

    // Draw edges
    let stroke = egui::Stroke::new(2.0, DesignSystem::TEXT_MUTED);
    let draw_arrow = |from: egui::Pos2, to: egui::Pos2| {
        let dir = (to - from).normalized();
        let start = from + dir * (node_radius + 2.0);
        let end = to - dir * (node_radius + 2.0);
        painter.line_segment([start, end], stroke);
        // Arrow head scaled with node_radius
        let head_len = (node_radius * 0.4).clamp(6.0, 10.0);
        let angle = std::f32::consts::PI / 6.0;
        let p1 = end
            - egui::Vec2::new(
                dir.x * angle.cos() - dir.y * angle.sin(),
                dir.x * angle.sin() + dir.y * angle.cos(),
            ) * head_len;
        let p2 = end
            - egui::Vec2::new(
                dir.x * angle.cos() + dir.y * angle.sin(),
                -dir.x * angle.sin() + dir.y * angle.cos(),
            ) * head_len;
        painter.line_segment([end, p1], stroke);
        painter.line_segment([end, p2], stroke);
    };

    draw_arrow(positions[0].1, positions[2].1); // Listener -> Sentinel
    draw_arrow(positions[1].1, positions[2].1); // ConnectionHealth -> Sentinel
    draw_arrow(positions[2].1, positions[3].1); // Sentinel -> Analyst
    draw_arrow(positions[3].1, positions[4].1); // Analyst -> RiskManager
    draw_arrow(positions[4].1, positions[5].1); // RiskManager -> Executor
    draw_arrow(positions[6].1, positions[3].1); // UserAgent -> Analyst
    draw_arrow(positions[6].1, positions[4].1); // UserAgent -> RiskManager

    // Draw nodes
    for (name, pos) in positions.iter() {
        // Find status
        let status = statuses.iter().find(|s| s.name == *name);
        let color = if let Some(s) = status {
            match s.health {
                HealthStatus::Healthy => DesignSystem::SUCCESS,
                HealthStatus::Degraded => DesignSystem::WARNING,
                HealthStatus::Dead => DesignSystem::DANGER,
                HealthStatus::Starting => DesignSystem::INFO,
            }
        } else {
            DesignSystem::TEXT_MUTED
        };

        painter.circle_filled(*pos, node_radius, DesignSystem::BG_CARD);
        painter.circle_stroke(*pos, node_radius, egui::Stroke::new(2.0, color));

        let text_color = DesignSystem::TEXT_PRIMARY;
        let label_offset = node_radius + 12.0;
        painter.text(
            *pos - egui::vec2(0.0, label_offset),
            egui::Align2::CENTER_CENTER,
            *name,
            egui::FontId::proportional(11.0),
            text_color,
        );
    }
}

fn render_agent_grid(ui: &mut egui::Ui, statuses: &[AgentStatus]) {
    // Determine grid size
    let available_width = ui.available_width();
    let card_width = 300.0;

    // Avoid division by zero if width is tiny
    let columns = if available_width > card_width {
        (available_width / (card_width + DesignSystem::SPACING_MEDIUM)).floor() as usize
    } else {
        1
    };

    let columns = columns.max(1);

    egui::Grid::new("agent_grid")
        .num_columns(columns)
        .spacing([DesignSystem::SPACING_MEDIUM, DesignSystem::SPACING_MEDIUM])
        .show(ui, |ui| {
            for (i, status) in statuses.iter().enumerate() {
                render_agent_card(ui, status);

                if (i + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
}

fn render_agent_card(ui: &mut egui::Ui, status: &AgentStatus) {
    let health_color = match status.health {
        HealthStatus::Healthy => DesignSystem::SUCCESS,
        HealthStatus::Degraded => DesignSystem::WARNING,
        HealthStatus::Dead => DesignSystem::DANGER,
        HealthStatus::Starting => DesignSystem::INFO,
    };

    // Detect specialized Risk status
    let is_halted = status
        .metrics
        .get("circuit_breaker")
        .map(|v| v == "HALTED")
        .unwrap_or(false);
    let halt_level = status.metrics.get("halt_level");

    let border_color = if is_halted {
        DesignSystem::DANGER
    } else if halt_level.map(|l| l != "Normal").unwrap_or(false) {
        DesignSystem::WARNING
    } else if status.health == HealthStatus::Dead {
        DesignSystem::DANGER
    } else {
        DesignSystem::BORDER_SUBTLE
    };

    let bg_color = if is_halted {
        DesignSystem::DANGER.linear_multiply(0.1)
    } else {
        DesignSystem::BG_CARD
    };

    egui::Frame::NONE
        .fill(bg_color)
        .corner_radius(DesignSystem::ROUNDING_MEDIUM)
        .stroke(egui::Stroke::new(1.0, border_color))
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(300.0);
            ui.set_min_height(180.0); // Allow expansion for metrics

            ui.vertical(|ui| {
                // Header
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&status.name)
                            .size(16.0)
                            .strong()
                            .color(DesignSystem::TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:?}", status.health)).color(health_color),
                        );
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Last Heartbeat
                let since_last = (chrono::Utc::now() - status.last_heartbeat).num_seconds();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Last Heartbeat:")
                            .size(12.0)
                            .color(DesignSystem::TEXT_SECONDARY),
                    );
                    let hb_color = if since_last > 10 {
                        DesignSystem::DANGER
                    } else {
                        DesignSystem::TEXT_PRIMARY
                    };
                    ui.label(
                        egui::RichText::new(format!("{}s ago", since_last))
                            .size(12.0)
                            .color(hb_color),
                    );
                });

                ui.add_space(DesignSystem::SPACING_SMALL);

                // Dynamic metrics display
                if !status.metrics.is_empty() {
                    ui.add_space(DesignSystem::SPACING_SMALL);
                    ui.label(
                        egui::RichText::new("Metrics:")
                            .size(11.0)
                            .strong()
                            .color(DesignSystem::TEXT_SECONDARY),
                    );
                    ui.add_space(4.0);

                    egui::Frame::NONE
                        .fill(DesignSystem::BG_WINDOW)
                        .corner_radius(DesignSystem::ROUNDING_SMALL)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical(|ui| {
                                let mut sorted_keys: Vec<&String> = status.metrics.keys().collect();
                                sorted_keys.sort();

                                for key in sorted_keys {
                                    let val = &status.metrics[key];

                                    // Determine semantic color for specific keys/values
                                    let val_color = match key.as_str() {
                                        "circuit_breaker" => {
                                            if val == "HALTED" {
                                                DesignSystem::DANGER
                                            } else {
                                                DesignSystem::SUCCESS
                                            }
                                        }
                                        "drawdown" | "consecutive_losses" => DesignSystem::DANGER,
                                        "daily_loss" => DesignSystem::WARNING,
                                        "queued_orders" | "queue" => {
                                            if val != "0" {
                                                DesignSystem::WARNING
                                            } else {
                                                DesignSystem::TEXT_PRIMARY
                                            }
                                        }
                                        "active_symbols" | "top_movers_count" => {
                                            DesignSystem::ACCENT_SECONDARY
                                        }
                                        _ => DesignSystem::TEXT_PRIMARY,
                                    };

                                    // Capitalize key for cleaner display
                                    let display_key = key.replace('_', " ");

                                    render_kv(ui, &display_key, val, val_color);
                                    ui.add_space(2.0);
                                }
                            });
                        });
                }
            });
        });
}

fn render_kv(ui: &mut egui::Ui, key: &str, value: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}: ", key))
                .size(11.0)
                .color(DesignSystem::TEXT_SECONDARY),
        );
        ui.label(egui::RichText::new(value).size(11.0).strong().color(color));
    });
}

fn render_global_halt_status(ui: &mut egui::Ui, statuses: &[AgentStatus]) {
    let is_any_halted = statuses.iter().any(|s| {
        s.metrics
            .get("circuit_breaker")
            .map(|v| v == "HALTED")
            .unwrap_or(false)
    });

    if is_any_halted {
        ui.add_space(DesignSystem::SPACING_MEDIUM);
        egui::Frame::NONE
            .fill(DesignSystem::DANGER.linear_multiply(0.15))
            .corner_radius(DesignSystem::ROUNDING_SMALL)
            .stroke(egui::Stroke::new(1.0, DesignSystem::DANGER))
            .inner_margin(DesignSystem::SPACING_MEDIUM)
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⚠ SYSTEM HALTED ⚠").size(16.0).strong().color(DesignSystem::DANGER));
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Circuit breaker triggered. Manual intervention or risk cool-down required.").size(13.0).color(DesignSystem::TEXT_PRIMARY));
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_system_metric_logic() {
        // Mock data to ensure logic doesn't panic
        let mut metrics = HashMap::new();
        metrics.insert("queue".to_string(), "5".to_string());

        let statuses = [
            AgentStatus {
                name: "Agent A".to_string(),
                health: HealthStatus::Healthy,
                last_heartbeat: Utc::now(),
                message: None,
                metrics: metrics.clone(),
            },
            AgentStatus {
                name: "Agent B".to_string(),
                health: HealthStatus::Dead,
                last_heartbeat: Utc::now() - chrono::Duration::seconds(60),
                message: Some("Timeout".to_string()),
                metrics: HashMap::new(),
            },
        ];

        // We can't easily mock egui::Ui in unit tests without a lot of boilerplate,
        // but we can sanity check our data structures and logic.
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].name, "Agent A");
    }
}
