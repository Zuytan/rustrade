use crate::application::agents::user_agent::UserAgent;
use crate::application::monitoring::agent_status::{AgentStatus, HealthStatus};
use crate::interfaces::design_system::DesignSystem;
use eframe::egui;
use std::sync::OnceLock;

pub static TOKIO_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

struct NodeDefinition {
    name: &'static str,
    label: &'static str,
    emoji: &'static str,
    col: usize,
    y_offset: f32,
    description: &'static str,
}

const NODES: &[NodeDefinition] = &[
    NodeDefinition {
        name: "Listener",
        label: "News Listener",
        emoji: "📡",
        col: 0,
        y_offset: -60.0,
        description: "Monitors external RSS and social media feeds. Applies NLP sentiment analysis to detect market-moving news and alert the Analyst.",
    },
    NodeDefinition {
        name: "ConnectionHealthService",
        label: "Conn Health",
        emoji: "🔌",
        col: 0,
        y_offset: 60.0,
        description: "Aggregates and broadcasts system-wide connectivity status (WebSocket feed, REST trade API) to safeguard automated operations.",
    },
    NodeDefinition {
        name: "Sentinel",
        label: "Sentinel Ingest",
        emoji: "⚡",
        col: 1,
        y_offset: 0.0,
        description: "Subscribes to live exchange streams. Normalizes high-frequency quote data and distributes it to the decision pipeline.",
    },
    NodeDefinition {
        name: "Analyst",
        label: "Analyst Brain",
        emoji: "🧠",
        col: 2,
        y_offset: -60.0,
        description: "Executes trading strategies (SMC/FVG regimes, statistical momentum), evaluates indicators, and submits trade proposals.",
    },
    NodeDefinition {
        name: "RiskManager",
        label: "Risk Manager",
        emoji: "🛡",
        col: 2,
        y_offset: 60.0,
        description: "Gatekeeper of the bot. Filters trade proposals against strict limits (max daily loss, drawdown, PDT, sector exposure).",
    },
    NodeDefinition {
        name: "Executor",
        label: "Executor Engine",
        emoji: "⚙",
        col: 3,
        y_offset: 0.0,
        description: "Transmits orders to brokers, manages trailing stop-losses, and reconciles pending orders with the exchange on startup.",
    },
    NodeDefinition {
        name: "UserAgent",
        label: "User Dashboard",
        emoji: "🖥",
        col: 4, // Special code for top position
        y_offset: 0.0,
        description: "Runs the graphical user interface. Visualizes system status, telemetry, metrics, activity logs, and trading configurations.",
    },
];

const CONNECTIONS: &[(&str, &str)] = &[
    ("Listener", "Sentinel"),
    ("ConnectionHealthService", "Sentinel"),
    ("Sentinel", "Analyst"),
    ("Sentinel", "RiskManager"),
    ("Analyst", "RiskManager"),
    ("RiskManager", "Executor"),
    ("UserAgent", "Analyst"),
    ("UserAgent", "RiskManager"),
    ("UserAgent", "Executor"),
];

/// Renders the Architecture & Metrics view
pub fn render_architecture_view(ui: &mut egui::Ui, agent: &UserAgent) {
    ui.vertical(|ui| {
        // Header
        ui.add_space(DesignSystem::SPACING_MEDIUM);
        ui.heading(
            egui::RichText::new(agent.i18n.t("arch_title"))
                .size(24.0)
                .strong()
                .color(DesignSystem::TEXT_PRIMARY),
        );
        ui.add_space(DesignSystem::SPACING_SMALL);
        ui.separator();
        ui.add_space(DesignSystem::SPACING_MEDIUM);

        // Fetch metrics from registry (UI thread safe)
        let registry = agent.client.agent_registry();

        // Throttled heartbeat registration for UserAgent
        let last_ua_hb_id = ui.make_persistent_id("last_user_agent_heartbeat");
        let last_ua_hb: Option<f64> = ui.data(|d| d.get_temp(last_ua_hb_id));
        let time = ui.ctx().input(|i| i.time);
        if last_ua_hb.is_none_or(|hb| time - hb > 2.0) {
            ui.data_mut(|d| d.insert_temp(last_ua_hb_id, time));
            if let Some(handle) = TOKIO_HANDLE.get() {
                let registry_clone = registry.clone();
                handle.spawn(async move {
                    registry_clone
                        .update_heartbeat("UserAgent", HealthStatus::Healthy)
                        .await;
                });
            }
        }

        let status_map = registry.get_all_sync();

        // Sort for stable display
        let mut statuses: Vec<AgentStatus> = status_map.values().cloned().collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));

        // 1. System Overview Metrics (Compact pills/badges)
        render_system_metrics(ui, &statuses, &agent.i18n);

        // 1.5 Global Halt Status
        render_global_halt_status(ui, &statuses, &agent.i18n);

        ui.add_space(DesignSystem::SPACING_MEDIUM);

        // 2. Agent Graph (Enclosed in a Card)
        crate::interfaces::components::card::Card::new().show(ui, |ui| {
            render_agent_graph(ui, &statuses, &agent.i18n);
        });

        ui.add_space(DesignSystem::SPACING_LARGE);

        // 3. Detail Inspector Section
        ui.heading(
            egui::RichText::new(agent.i18n.t("arch_inspector_title"))
                .size(18.0)
                .strong()
                .color(DesignSystem::TEXT_PRIMARY),
        );
        ui.add_space(DesignSystem::SPACING_MEDIUM);

        let selected_agent_id = egui::Id::new("selected_agent_id");
        let selected_agent = ui
            .data(|d| d.get_temp::<String>(selected_agent_id))
            .filter(|s| !s.is_empty());

        if let Some(agent_name) = selected_agent {
            if let Some(status) = statuses.iter().find(|s| s.name == agent_name) {
                render_agent_detail_panel(ui, status, &agent.i18n);
            } else {
                let offline_msg = agent
                    .i18n
                    .tf("arch_offline_inspector", &[("name", &agent_name)]);
                render_empty_inspector(ui, &offline_msg);
            }
        } else {
            render_empty_inspector(ui, agent.i18n.t("arch_empty_inspector"));
        }
    });
}

fn render_system_metrics(
    ui: &mut egui::Ui,
    statuses: &[AgentStatus],
    i18n: &crate::infrastructure::i18n::I18nService,
) {
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
        ui.label(
            egui::RichText::new(i18n.t("arch_system_status"))
                .size(11.0)
                .strong()
                .color(DesignSystem::TEXT_MUTED),
        );

        render_status_pill_inline(
            ui,
            &format!("{}: {}", i18n.t("arch_total"), total_agents),
            DesignSystem::TEXT_PRIMARY,
            DesignSystem::BG_CARD,
        );

        render_status_pill_inline(
            ui,
            &format!("{}: {}", i18n.t("arch_healthy"), healthy_count),
            DesignSystem::SUCCESS,
            DesignSystem::SUCCESS.linear_multiply(0.12),
        );

        if degraded_count > 0 {
            render_status_pill_inline(
                ui,
                &format!("{}: {}", i18n.t("arch_degraded"), degraded_count),
                DesignSystem::WARNING,
                DesignSystem::WARNING.linear_multiply(0.12),
            );
        }

        if dead_count > 0 {
            render_status_pill_inline(
                ui,
                &format!("{}: {}", i18n.t("arch_dead"), dead_count),
                DesignSystem::DANGER,
                DesignSystem::DANGER.linear_multiply(0.12),
            );
        } else {
            render_status_pill_inline(
                ui,
                &format!("{}: 0", i18n.t("arch_dead")),
                DesignSystem::TEXT_MUTED,
                DesignSystem::BORDER_SUBTLE.linear_multiply(0.5),
            );
        }
    });
}

fn render_status_pill_inline(
    ui: &mut egui::Ui,
    text: &str,
    text_color: egui::Color32,
    bg_color: egui::Color32,
) {
    egui::Frame::NONE
        .fill(bg_color)
        .corner_radius(DesignSystem::ROUNDING_SMALL)
        .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(11.0)
                    .strong()
                    .color(text_color),
            );
        });
}

fn get_bezier_points(p0: egui::Pos2, p3: egui::Pos2) -> Vec<egui::Pos2> {
    let p1 = egui::pos2(p0.x + (p3.x - p0.x) * 0.5, p0.y);
    let p2 = egui::pos2(p0.x + (p3.x - p0.x) * 0.5, p3.y);
    let steps = 20;
    let mut points = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let mt = 1.0 - t;
        let x = mt.powi(3) * p0.x
            + 3.0 * mt.powi(2) * t * p1.x
            + 3.0 * mt * t.powi(2) * p2.x
            + t.powi(3) * p3.x;
        let y = mt.powi(3) * p0.y
            + 3.0 * mt.powi(2) * t * p1.y
            + 3.0 * mt * t.powi(2) * p2.y
            + t.powi(3) * p3.y;
        points.push(egui::pos2(x, y));
    }
    points
}

fn get_bezier_point(p0: egui::Pos2, p3: egui::Pos2, t: f32) -> egui::Pos2 {
    let p1 = egui::pos2(p0.x + (p3.x - p0.x) * 0.5, p0.y);
    let p2 = egui::pos2(p0.x + (p3.x - p0.x) * 0.5, p3.y);
    let mt = 1.0 - t;
    let x = mt.powi(3) * p0.x
        + 3.0 * mt.powi(2) * t * p1.x
        + 3.0 * mt * t.powi(2) * p2.x
        + t.powi(3) * p3.x;
    let y = mt.powi(3) * p0.y
        + 3.0 * mt.powi(2) * t * p1.y
        + 3.0 * mt * t.powi(2) * p2.y
        + t.powi(3) * p3.y;
    egui::pos2(x, y)
}

fn translate_health(health: HealthStatus, i18n: &crate::infrastructure::i18n::I18nService) -> &str {
    match health {
        HealthStatus::Healthy => i18n.t("arch_healthy"),
        HealthStatus::Degraded => i18n.t("arch_degraded"),
        HealthStatus::Dead => i18n.t("arch_dead"),
        HealthStatus::Starting => i18n.t("arch_starting"),
    }
}

fn render_agent_graph(
    ui: &mut egui::Ui,
    statuses: &[AgentStatus],
    i18n: &crate::infrastructure::i18n::I18nService,
) {
    let width = ui.available_width().max(800.0);
    let (response, painter) = ui.allocate_painter(egui::vec2(width, 320.0), egui::Sense::click());
    let rect = response.rect;

    let center_x = rect.center().x;
    let x_padding = 50.0;
    let y_padding = 40.0;
    let col_width = (rect.width() - 2.0 * x_padding) / 3.0;

    let get_node_pos = |node: &NodeDefinition| -> egui::Pos2 {
        if node.col == 4 {
            egui::pos2(center_x, rect.top() + y_padding + 22.0)
        } else {
            let x = rect.left() + x_padding + node.col as f32 * col_width;
            let graph_center_y = rect.top() + y_padding + (rect.height() - y_padding * 2.0) * 0.55;
            egui::pos2(x, graph_center_y + node.y_offset)
        }
    };

    let selected_agent_id = egui::Id::new("selected_agent_id");
    let mut selected_agent = ui
        .data(|d| d.get_temp::<String>(selected_agent_id))
        .filter(|s| !s.is_empty());

    // Manual click and hover handling via local painter response
    let mut hovered_node = None;
    if let Some(pos) = response.hover_pos() {
        for node_def in NODES {
            let pos_center = get_node_pos(node_def);
            let node_rect = egui::Rect::from_center_size(pos_center, egui::vec2(150.0, 56.0));
            if node_rect.contains(pos) {
                hovered_node = Some(node_def.name);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                break;
            }
        }
    }

    let mut clicked_node = None;
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let mut clicked_any_node = false;
        for node_def in NODES {
            let pos_center = get_node_pos(node_def);
            let node_rect = egui::Rect::from_center_size(pos_center, egui::vec2(150.0, 56.0));
            if node_rect.contains(pos) {
                tracing::info!(
                    "DEBUG: Node {} clicked via local intersection!",
                    node_def.name
                );
                clicked_node = Some(node_def.name);
                clicked_any_node = true;
                break;
            }
        }
        if !clicked_any_node {
            tracing::info!("DEBUG: Background clicked via local intersection.");
            selected_agent = None;
            ui.data_mut(|d| d.insert_temp(selected_agent_id, String::new()));
        }
    }

    if let Some(name) = clicked_node {
        if selected_agent.as_deref() == Some(name) {
            selected_agent = None;
            ui.data_mut(|d| d.insert_temp(selected_agent_id, String::new()));
        } else {
            selected_agent = Some(name.to_string());
            ui.data_mut(|d| d.insert_temp(selected_agent_id, name.to_string()));
        }
    }

    // 1. Draw Columns in Background
    let col_titles = [
        i18n.t("col_input"),
        i18n.t("col_ingestion"),
        i18n.t("col_decision"),
        i18n.t("col_execution"),
    ];
    for (c, &title) in col_titles.iter().enumerate() {
        let x = rect.left() + x_padding + c as f32 * col_width;
        let col_rect = egui::Rect::from_min_max(
            egui::pos2(x - col_width / 2.0 + 8.0, rect.top() + 85.0),
            egui::pos2(x + col_width / 2.0 - 8.0, rect.bottom() - 10.0),
        );

        painter.rect_filled(
            col_rect,
            DesignSystem::ROUNDING_SMALL,
            egui::Color32::from_rgba_unmultiplied(18, 22, 33, 40),
        );
        painter.rect_stroke(
            col_rect,
            DesignSystem::ROUNDING_SMALL,
            egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(31, 41, 55, 60)),
            egui::StrokeKind::Outside,
        );

        painter.text(
            egui::pos2(x, rect.top() + 97.0),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(10.0),
            DesignSystem::TEXT_MUTED,
        );
    }

    // Draw top Monitoring section frame
    let mon_rect = egui::Rect::from_min_max(
        egui::pos2(center_x - 100.0, rect.top() + 5.0),
        egui::pos2(center_x + 100.0, rect.top() + 75.0),
    );
    painter.rect_filled(
        mon_rect,
        DesignSystem::ROUNDING_SMALL,
        egui::Color32::from_rgba_unmultiplied(18, 22, 33, 25),
    );
    painter.rect_stroke(
        mon_rect,
        DesignSystem::ROUNDING_SMALL,
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(31, 41, 55, 40)),
        egui::StrokeKind::Outside,
    );
    painter.text(
        egui::pos2(center_x, rect.top() + 15.0),
        egui::Align2::CENTER_CENTER,
        i18n.t("arch_system_monitoring"),
        egui::FontId::proportional(9.0),
        DesignSystem::TEXT_MUTED,
    );

    // 2. Draw Connections
    for &(from_name, to_name) in CONNECTIONS {
        let from_node = NODES.iter().find(|n| n.name == from_name);
        let to_node = NODES.iter().find(|n| n.name == to_name);

        if let (Some(f), Some(t)) = (from_node, to_node) {
            let from_pos = get_node_pos(f);
            let to_pos = get_node_pos(t);

            let from_status = statuses.iter().find(|s| s.name == from_name);
            let to_status = statuses.iter().find(|s| s.name == to_name);

            let from_health = from_status
                .map(|s| s.health)
                .unwrap_or(HealthStatus::Healthy);
            let to_health = to_status.map(|s| s.health).unwrap_or(HealthStatus::Healthy);

            let stroke_color = if from_health == HealthStatus::Dead
                || to_health == HealthStatus::Dead
            {
                DesignSystem::DANGER.linear_multiply(0.4)
            } else if from_health == HealthStatus::Degraded || to_health == HealthStatus::Degraded {
                DesignSystem::WARNING.linear_multiply(0.4)
            } else {
                DesignSystem::ACCENT_PRIMARY.linear_multiply(0.25)
            };

            let stroke_width =
                if from_health == HealthStatus::Dead || to_health == HealthStatus::Dead {
                    1.0
                } else {
                    1.5
                };

            let points = get_bezier_points(from_pos, to_pos);
            let stroke = egui::Stroke::new(stroke_width, stroke_color);
            painter.line(points, stroke);

            // Animated Particles
            let time = ui.ctx().input(|i| i.time) as f32;
            if from_health == HealthStatus::Healthy && to_health == HealthStatus::Healthy {
                for p_idx in 0..2 {
                    let offset = (time * 0.4 + p_idx as f32 * 0.5).fract();
                    let particle_pos = get_bezier_point(from_pos, to_pos, offset);

                    let alpha = if offset < 0.2 {
                        offset / 0.2
                    } else if offset > 0.8 {
                        (1.0 - offset) / 0.2
                    } else {
                        1.0
                    };

                    let p_color = DesignSystem::ACCENT_SECONDARY.linear_multiply(alpha * 0.8);
                    painter.circle_filled(particle_pos, 2.5, p_color);
                }
            }
        }
    }

    // 3. Draw Nodes (interactive cards)
    for node_def in NODES {
        let pos = get_node_pos(node_def);
        let node_rect = egui::Rect::from_center_size(pos, egui::vec2(150.0, 56.0));

        let is_selected = selected_agent.as_deref() == Some(node_def.name);
        let is_hovered = hovered_node == Some(node_def.name);

        let status = statuses.iter().find(|s| s.name == node_def.name);
        let (health_status, status_color) = if let Some(s) = status {
            let color = match s.health {
                HealthStatus::Healthy => DesignSystem::SUCCESS,
                HealthStatus::Degraded => DesignSystem::WARNING,
                HealthStatus::Dead => DesignSystem::DANGER,
                HealthStatus::Starting => DesignSystem::INFO,
            };
            (s.health, color)
        } else {
            (HealthStatus::Dead, DesignSystem::TEXT_MUTED)
        };

        let time = ui.ctx().input(|i| i.time) as f32;
        let pulse = if health_status == HealthStatus::Healthy {
            (time * 3.0).sin().abs() * 0.15 + 0.85
        } else if health_status == HealthStatus::Dead {
            (time * 6.0).sin().abs() * 0.3 + 0.7
        } else {
            1.0
        };

        let border_color = if is_selected {
            DesignSystem::ACCENT_PRIMARY
        } else if is_hovered {
            status_color
        } else {
            DesignSystem::BORDER_SUBTLE
        };

        let border_thickness = if is_selected { 2.0 } else { 1.0 };

        // Outer glow
        if is_selected || is_hovered {
            let glow_color = if is_selected {
                DesignSystem::ACCENT_PRIMARY
            } else {
                status_color
            };
            for j in 1..=3 {
                let expansion = j as f32 * 2.0;
                let alpha = 0.15 / j as f32;
                let glow_rect = node_rect.expand(expansion);
                painter.rect_stroke(
                    glow_rect,
                    DesignSystem::ROUNDING_SMALL + expansion * 0.5,
                    egui::Stroke::new(1.0, glow_color.linear_multiply(alpha)),
                    egui::StrokeKind::Outside,
                );
            }
        }

        // Draw card background
        let bg_fill = if is_hovered {
            DesignSystem::BG_CARD_HOVER
        } else {
            DesignSystem::BG_CARD
        };
        painter.rect_filled(node_rect, DesignSystem::ROUNDING_SMALL, bg_fill);

        if is_selected {
            painter.rect_filled(
                node_rect,
                DesignSystem::ROUNDING_SMALL,
                DesignSystem::ACCENT_PRIMARY.linear_multiply(0.06),
            );
        }

        // Stroke border
        painter.rect_stroke(
            node_rect,
            DesignSystem::ROUNDING_SMALL,
            egui::Stroke::new(border_thickness, border_color),
            egui::StrokeKind::Outside,
        );

        // Status indicator dot
        let dot_center = egui::pos2(node_rect.left() + 15.0, node_rect.center().y);
        let base_dot_radius = 4.0;
        let dot_radius = if health_status == HealthStatus::Healthy {
            base_dot_radius + (time * 3.5).sin().abs() * 1.5
        } else if health_status == HealthStatus::Dead {
            base_dot_radius + (time * 7.0).sin().abs() * 2.0
        } else {
            base_dot_radius
        };

        painter.circle_filled(
            dot_center,
            dot_radius * 1.5,
            status_color.linear_multiply(0.2 * pulse),
        );
        painter.circle_filled(dot_center, base_dot_radius, status_color);

        // Text title
        let title_pos = egui::pos2(node_rect.left() + 26.0, node_rect.top() + 10.0);
        let label_key = format!("agent_{}_label", node_def.name);
        let node_label = i18n.t(&label_key);
        let label = if node_label == label_key {
            node_def.label
        } else {
            node_label
        };
        painter.text(
            title_pos,
            egui::Align2::LEFT_TOP,
            format!("{} {}", node_def.emoji, label),
            egui::FontId::proportional(11.5),
            DesignSystem::TEXT_PRIMARY,
        );

        // Subtext (Health & Heartbeat latency)
        let sub_pos = egui::pos2(node_rect.left() + 26.0, node_rect.bottom() - 10.0);
        let sub_text = if let Some(s) = status {
            let since_last = (chrono::Utc::now() - s.last_heartbeat).num_seconds().max(0);
            let health_str = translate_health(s.health, i18n);
            let secs_ago = i18n.tf("arch_seconds_ago", &[("secs", &since_last.to_string())]);
            format!("{} ({})", health_str, secs_ago)
        } else {
            i18n.t("arch_offline").to_string()
        };
        painter.text(
            sub_pos,
            egui::Align2::LEFT_BOTTOM,
            sub_text,
            egui::FontId::proportional(9.0),
            DesignSystem::TEXT_SECONDARY,
        );
    }
}

fn render_agent_detail_panel(
    ui: &mut egui::Ui,
    status: &AgentStatus,
    i18n: &crate::infrastructure::i18n::I18nService,
) {
    let health_color = match status.health {
        HealthStatus::Healthy => DesignSystem::SUCCESS,
        HealthStatus::Degraded => DesignSystem::WARNING,
        HealthStatus::Dead => DesignSystem::DANGER,
        HealthStatus::Starting => DesignSystem::INFO,
    };

    crate::interfaces::components::card::Card::new()
        .active(true)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.columns(2, |cols| {
                // Column 0: Agent Info
                cols[0].vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&status.name)
                                .size(20.0)
                                .strong()
                                .color(DesignSystem::TEXT_PRIMARY),
                        );
                        ui.add_space(8.0);
                        crate::interfaces::components::metrics::render_status_pill(
                            ui,
                            translate_health(status.health, i18n),
                            health_color,
                        );
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    let node_def = NODES.iter().find(|n| n.name == status.name);
                    if let Some(def) = node_def {
                        let desc_key = format!("agent_{}_desc", def.name);
                        let node_desc = i18n.t(&desc_key);
                        let description = if node_desc == desc_key {
                            def.description
                        } else {
                            node_desc
                        };
                        ui.label(
                            egui::RichText::new(description)
                                .size(12.0)
                                .color(DesignSystem::TEXT_SECONDARY),
                        );
                        ui.add_space(10.0);
                    }

                    let since_last = (chrono::Utc::now() - status.last_heartbeat)
                        .num_seconds()
                        .max(0);
                    let hb_color = if since_last > 10 {
                        DesignSystem::DANGER
                    } else {
                        DesignSystem::TEXT_PRIMARY
                    };

                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(i18n.t("arch_last_heartbeat"))
                                .size(13.0)
                                .color(DesignSystem::TEXT_SECONDARY),
                        );
                        let secs_ago =
                            i18n.tf("arch_seconds_ago", &[("secs", &since_last.to_string())]);
                        ui.label(
                            egui::RichText::new(secs_ago)
                                .size(13.0)
                                .strong()
                                .color(hb_color),
                        );
                    });

                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(i18n.t("arch_utc_timestamp"))
                                .size(11.0)
                                .color(DesignSystem::TEXT_MUTED),
                        );
                        ui.label(
                            egui::RichText::new(
                                status
                                    .last_heartbeat
                                    .format("%Y-%m-%d %H:%M:%S UTC")
                                    .to_string(),
                            )
                            .size(11.0)
                            .color(DesignSystem::TEXT_MUTED),
                        );
                    });

                    if let Some(ref msg) = status.message {
                        ui.add_space(DesignSystem::SPACING_SMALL);
                        ui.label(
                            egui::RichText::new(i18n.t("arch_status_message"))
                                .size(12.0)
                                .strong()
                                .color(DesignSystem::TEXT_SECONDARY),
                        );
                        egui::Frame::NONE
                            .fill(DesignSystem::BG_INPUT)
                            .corner_radius(DesignSystem::ROUNDING_SMALL)
                            .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    egui::RichText::new(msg)
                                        .size(12.0)
                                        .color(DesignSystem::TEXT_PRIMARY),
                                );
                            });
                    }
                });

                // Column 1: Metrics
                cols[1].vertical(|ui| {
                    ui.label(
                        egui::RichText::new(i18n.t("arch_diagnostics_title"))
                            .size(13.0)
                            .strong()
                            .color(DesignSystem::TEXT_SECONDARY),
                    );
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    if status.metrics.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                egui::RichText::new(i18n.t("arch_no_metrics"))
                                    .size(12.0)
                                    .italics()
                                    .color(DesignSystem::TEXT_MUTED),
                            );
                        });
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(140.0)
                            .show(ui, |ui| {
                                egui::Grid::new("agent_detail_metrics")
                                    .num_columns(2)
                                    .spacing([12.0, 6.0])
                                    .show(ui, |ui| {
                                        let mut sorted_keys: Vec<&String> =
                                            status.metrics.keys().collect();
                                        sorted_keys.sort();

                                        for key in sorted_keys {
                                            let val = &status.metrics[key];
                                            let display_key = key.replace('_', " ");

                                            ui.label(
                                                egui::RichText::new(display_key)
                                                    .size(12.0)
                                                    .color(DesignSystem::TEXT_SECONDARY),
                                            );

                                            let val_color = match key.as_str() {
                                                "circuit_breaker" => {
                                                    if val == "HALTED" {
                                                        DesignSystem::DANGER
                                                    } else {
                                                        DesignSystem::SUCCESS
                                                    }
                                                }
                                                "drawdown" | "consecutive_losses" => {
                                                    DesignSystem::DANGER
                                                }
                                                "daily_loss" => DesignSystem::WARNING,
                                                "queued_orders" | "queue" => {
                                                    if val != "0" {
                                                        DesignSystem::WARNING
                                                    } else {
                                                        DesignSystem::TEXT_PRIMARY
                                                    }
                                                }
                                                _ => DesignSystem::ACCENT_SECONDARY,
                                            };

                                            ui.label(
                                                egui::RichText::new(val)
                                                    .size(12.0)
                                                    .strong()
                                                    .color(val_color),
                                            );
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                });
            });
        });
}

fn render_empty_inspector(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(DesignSystem::BG_CARD)
        .corner_radius(DesignSystem::ROUNDING_MEDIUM)
        .stroke(egui::Stroke::new(1.0, DesignSystem::BORDER_SUBTLE))
        .inner_margin(24.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("🔍")
                            .size(24.0)
                            .color(DesignSystem::TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(text)
                            .size(13.0)
                            .color(DesignSystem::TEXT_SECONDARY),
                    );
                });
            });
        });
}

fn render_global_halt_status(
    ui: &mut egui::Ui,
    statuses: &[AgentStatus],
    i18n: &crate::infrastructure::i18n::I18nService,
) {
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
                        ui.label(
                            egui::RichText::new(i18n.t("arch_global_halt"))
                                .size(16.0)
                                .strong()
                                .color(DesignSystem::DANGER),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(i18n.t("arch_global_halt_desc"))
                                .size(13.0)
                                .color(DesignSystem::TEXT_PRIMARY),
                        );
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

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].name, "Agent A");
    }
}
