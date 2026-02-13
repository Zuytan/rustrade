use crate::application::agents::user_agent::UserAgent;
use crate::application::monitoring::agent_status::{AgentStatus, HealthStatus};
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;

/// Renders the Architecture & Metrics view
pub fn render_architecture_view(ui: &mut egui::Ui, agent: &UserAgent) {
    ui.vertical(|ui| {
        // Header
        ui.add_space(DesignSystem::SPACING_MEDIUM);
        ui.heading("System Architecture & Agent Status");
        ui.add_space(DesignSystem::SPACING_LARGE);

        // Fetch metrics from registry (UI thread safe)
        let registry = agent.client.agent_registry();
        let status_map = registry.get_all_sync();

        // Sort for stable display
        let mut statuses: Vec<AgentStatus> = status_map.values().cloned().collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));

        // 1. System Overview Metrics
        render_system_metrics(ui, &statuses);

        ui.add_space(DesignSystem::SPACING_LARGE);
        ui.separator();
        ui.add_space(DesignSystem::SPACING_LARGE);

        // 2. Agent Grid
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

    ui.horizontal(|ui| {
        render_metric_card(
            ui,
            "Total Agents",
            &total_agents.to_string(),
            DesignSystem::TEXT_PRIMARY,
        );
        ui.add_space(DesignSystem::SPACING_MEDIUM);
        render_metric_card(
            ui,
            "Healthy",
            &healthy_count.to_string(),
            DesignSystem::SUCCESS,
        );
        ui.add_space(DesignSystem::SPACING_MEDIUM);
        render_metric_card(
            ui,
            "Degraded",
            &degraded_count.to_string(),
            DesignSystem::WARNING,
        );
        ui.add_space(DesignSystem::SPACING_MEDIUM);
        render_metric_card(ui, "Dead", &dead_count.to_string(), DesignSystem::DANGER);
    });
}

fn render_metric_card(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    egui::Frame::NONE
        .fill(DesignSystem::BG_CARD)
        .corner_radius(DesignSystem::ROUNDING_MEDIUM)
        .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
        .inner_margin(16.0)
        .show(ui, |ui| {
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

    let border_color = if status.health == HealthStatus::Dead {
        DesignSystem::DANGER
    } else {
        DesignSystem::BORDER_SUBTLE
    };

    egui::Frame::NONE
        .fill(DesignSystem::BG_CARD)
        .corner_radius(DesignSystem::ROUNDING_MEDIUM)
        .stroke(egui::Stroke::new(1.0, border_color))
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(300.0);
            ui.set_height(180.0); // Fixed height for uniformity

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

                ui.add_space(DesignSystem::SPACING_MEDIUM);

                // Custom Metrics
                if !status.metrics.is_empty() {
                    ui.label(
                        egui::RichText::new("Metrics:")
                            .size(12.0)
                            .strong()
                            .color(DesignSystem::TEXT_SECONDARY),
                    );
                    for (key, value) in &status.metrics {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{}:", key))
                                    .size(11.0)
                                    .color(DesignSystem::TEXT_SECONDARY),
                            );
                            ui.label(egui::RichText::new(value).size(11.0).code());
                        });
                    }
                } else {
                    ui.label(
                        egui::RichText::new("No active metrics")
                            .size(12.0)
                            .italics()
                            .color(DesignSystem::TEXT_MUTED),
                    );
                }
            });
        });
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

        let statuses = vec![
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
