// SPDX-License-Identifier: MIT OR Apache-2.0

//! UI rendering for the TUI.
//!
//! Uses ratatui widgets: Gauge, LineGauge, Sparkline, Table, Tabs,
//! Scrollbar, Paragraph, Block, BarChart.

use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Gauge, LineGauge, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Sparkline, Table, Tabs,
};

use crate::tui::App;

/// Tab names.
const TAB_NAMES: &[&str] = &["Dashboard", "Interfaces", "Bonds", "Logs"];

/// Color a health score: green >70, yellow 40-70, red <40.
fn health_color(score: f64) -> Color {
    if score >= 70.0 {
        Color::Green
    } else if score >= 40.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Format seconds into human-readable uptime.
fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

/// Format bytes into human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Render the full UI.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header + tabs
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Status bar
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_content(frame, app, chunks[1]);
    render_status_bar(frame, app, chunks[2]);
}

/// Render the header and tab bar.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(2)])
        .split(area);

    // Title bar with loading indicator
    let title_text = if app.is_loading {
        " NetFusion ⟳ Loading..."
    } else {
        " NetFusion"
    };
    let title = Paragraph::new(title_text)
        .style(Style::default().fg(Color::Cyan).bg(Color::Black).bold());
    frame.render_widget(title, chunks[0]);

    // Tabs
    let tabs = Tabs::new(TAB_NAMES.iter().copied())
        .select(app.selected_tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .divider("|");
    frame.render_widget(tabs, chunks[1]);
}

/// Render the main content area based on selected tab.
fn render_content(frame: &mut Frame, app: &App, area: Rect) {
    match app.selected_tab {
        0 => render_dashboard(frame, app, area),
        1 => render_interfaces(frame, app, area),
        2 => render_bonds(frame, app, area),
        3 => render_logs(frame, app, area),
        _ => {}
    }
}

/// Dashboard view — health gauges, sparkline, interface overview.
fn render_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // System status
            Constraint::Length(9),  // Health breakdown + sparkline
            Constraint::Min(0),     // Interface health overview
        ])
        .split(area);

    // Connection + system status
    let status_text = if let Some(ref status) = app.status {
        let uptime = format_uptime(status.uptime_secs);
        let failover = if status.failover_active { " | Failover Active" } else { "" };
        let profile = status
            .active_profile
            .as_ref()
            .map(|p| format!(" | Profile: {}", p))
            .unwrap_or_default();
        format!(
            "Interfaces: {} | Bonds: {} | Tunnels: {} | Uptime: {}{}{}",
            status.total_interfaces, status.active_bonds, status.connected_tunnels, uptime, failover, profile
        )
    } else if !app.connected {
        "Disconnected from daemon".into()
    } else {
        "Loading system status...".into()
    };
    let conn_color = if app.connected { Color::Green } else { Color::Red };
    let status_para = Paragraph::new(status_text).style(Style::default().fg(conn_color));
    frame.render_widget(
        Block::default().borders(Borders::ALL).title(" System "),
        chunks[0],
    );
    frame.render_widget(status_para, chunks[0]);

    // Health breakdown (left) + sparkline (right)
    let sub_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_health_breakdown(frame, app, sub_chunks[0]);
    render_health_sparkline(frame, app, sub_chunks[1]);

    // Interface health overview
    render_interface_health(frame, app, chunks[2]);
}

/// Render individual health component gauges.
fn render_health_breakdown(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(ref status) = app.status {
        if let Some(ref health) = status.health {
            let items = [
                ("Overall", health.overall),
                ("RTT", health.rtt),
                ("Jitter", health.jitter),
                ("Loss", health.loss),
                ("Throughput", health.throughput),
                ("Stability", health.stability),
            ];

            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Health Breakdown ");
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Length(1); items.len()])
                .split(inner);

            for (i, (label, score)) in items.iter().enumerate() {
                if i >= rows.len() {
                    break;
                }
                let color = health_color(*score);
                let gauge = LineGauge::default()
                    .filled_style(Style::default().fg(color))
                    .ratio(score / 100.0)
                    .label(format!("{:.0}", score));
                // Prepend label manually
                let label_line = format!(" {:<12}", label);
                let label_widget = Paragraph::new(label_line)
                    .style(Style::default().fg(Color::Gray));
                frame.render_widget(label_widget, rows[i]);
                // Render gauge in a sub-chunk offset by label width
                let gauge_area = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(13), Constraint::Min(0)])
                    .split(rows[i])[1];
                frame.render_widget(gauge, gauge_area);
            }
        }
    }
}

/// Render health history sparkline.
fn render_health_sparkline(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Health Trend ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let data: Vec<u64> = app
        .health_history
        .iter()
        .map(|v| (*v as u64).min(100))
        .collect();

    let sparkline = Sparkline::default()
        .data(&data)
        .style(Style::default().fg(Color::Cyan))
        .max(100);
    frame.render_widget(sparkline, inner);
}

/// Render per-interface health overview with colored gauges.
fn render_interface_health(frame: &mut Frame, app: &App, area: Rect) {
    if app.interfaces.is_empty() {
        let placeholder = Paragraph::new("No interfaces discovered")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(
            Block::default().borders(Borders::ALL).title(" Interface Health "),
            area,
        );
        frame.render_widget(placeholder, area);
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Interface Health ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let row_height = 1;
    let max_rows = inner.height as usize;
    let rows: Vec<_> = app
        .interfaces
        .iter()
        .filter(|i| i.health.is_some())
        .take(max_rows)
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(row_height as u16); rows.len()])
        .split(inner);

    for (i, iface) in rows.iter().enumerate() {
        if i >= chunks.len() {
            break;
        }
        if let Some(ref h) = iface.health {
            let color = health_color(h.overall);
            let label = format!(" {:<10} {:.0}/100", iface.name, h.overall);
            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(color))
                .ratio(h.overall / 100.0)
                .label(label);
            frame.render_widget(gauge, chunks[i]);
        }
    }
}

/// Interfaces tab — scrollable table with selection + detail panel.
fn render_interfaces(frame: &mut Frame, app: &App, area: Rect) {
    // Split: table top, detail panel bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height / 2 + 1),
            Constraint::Min(0),
        ])
        .split(area);

    // Store viewport height for scrollbar
    let available_height = (chunks[0].height.saturating_sub(1)) as usize; // -1 for header
    let content_len = app.interfaces.len();

    // Build table rows
    let rows: Vec<Row> = app
        .interfaces
        .iter()
        .map(|iface| {
            let state = match iface.link_state {
                netfusion_shared::types::LinkState::Up => "UP",
                netfusion_shared::types::LinkState::Down => "DOWN",
                netfusion_shared::types::LinkState::Unknown => "???",
            };
            let state_color = match iface.link_state {
                netfusion_shared::types::LinkState::Up => Color::Green,
                netfusion_shared::types::LinkState::Down => Color::Red,
                netfusion_shared::types::LinkState::Unknown => Color::Gray,
            };
            let speed = iface
                .speed_mbps
                .map(|s| format!("{} Mbps", s))
                .unwrap_or_else(|| "-".into());
            let health = iface
                .health
                .as_ref()
                .map(|h| format!("{:.0}", h.overall))
                .unwrap_or_else(|| "N/A".into());
            let health_color_val = iface
                .health
                .as_ref()
                .map(|h| health_color(h.overall))
                .unwrap_or(Color::Gray);
            Row::new(vec![
                iface.name.clone(),
                format!("{:?}", iface.if_type),
                state.to_string(),
                speed,
                health,
            ]).style(Style::default().fg(state_color).fg(health_color_val))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Length(7),
        ],
    )
    .header(
        Row::new(vec!["Interface", "Type", "State", "Speed", "Health"])
            .style(Style::default().fg(Color::Yellow).bold()),
    )
    .block(Block::default().borders(Borders::ALL).title(" Interfaces "))
    .highlight_symbol("> ")
    .row_highlight_style(Style::default().bg(Color::DarkGray).bold());

    frame.render_widget(table, chunks[0]);

    // Scrollbar
    let mut scroll_state = ScrollbarState::default()
        .content_length(content_len)
        .position(app.interface_scroll_pos);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        chunks[0],
        &mut scroll_state,
    );

    // Detail panel for selected interface
    render_interface_detail(frame, app, chunks[1]);
}

/// Render detail panel for the selected interface.
fn render_interface_detail(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Interface Detail ");
    let inner = block.inner(area);
    frame.render_widget(block.clone(), area);

    let Some(iface) = app.interfaces.get(app.selected_interface_idx) else {
        frame.render_widget(Paragraph::new("No interface selected"), inner);
        return;
    };

    let addr = iface
        .addresses
        .first()
        .map(|a| a.cidr.clone())
        .unwrap_or_else(|| "-".into());
    let gw = iface.gateway.as_deref().unwrap_or("-");
    let mac = iface.mac.as_deref().unwrap_or("-");
    let mtu = iface.mtu.to_string();
    let rx = format_bytes(iface.stats.rx_bytes);
    let tx = format_bytes(iface.stats.tx_bytes);
    let errors = iface.stats.rx_errors + iface.stats.tx_errors;
    let dropped = iface.stats.rx_dropped + iface.stats.tx_dropped;

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Address:  ", Style::default().fg(Color::Gray)),
            Span::raw(&addr),
        ]),
        Line::from(vec![
            Span::styled("Gateway:  ", Style::default().fg(Color::Gray)),
            Span::raw(gw),
        ]),
        Line::from(vec![
            Span::styled("MAC:      ", Style::default().fg(Color::Gray)),
            Span::raw(mac),
        ]),
        Line::from(vec![
            Span::styled("MTU:      ", Style::default().fg(Color::Gray)),
            Span::raw(&mtu),
        ]),
        Line::from(vec![
            Span::styled("RX:       ", Style::default().fg(Color::Gray)),
            Span::raw(&rx),
        ]),
        Line::from(vec![
            Span::styled("TX:       ", Style::default().fg(Color::Gray)),
            Span::raw(&tx),
        ]),
        Line::from(vec![
            Span::styled("Errors:   ", Style::default().fg(Color::Gray)),
            Span::raw(errors.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Dropped:  ", Style::default().fg(Color::Gray)),
            Span::raw(dropped.to_string()),
        ]),
    ];

    // Wireless info
    if let Some(ref wifi) = iface.wireless {
        lines.push(Line::from(vec![
            Span::styled("SSID:     ", Style::default().fg(Color::Gray)),
            Span::raw(wifi.ssid.as_deref().unwrap_or("-")),
        ]));
        let signal = wifi.signal_dbm.map(|s| s.to_string()).unwrap_or_else(|| "-".into());
        let quality = wifi.quality_percent.map(|q| format!("{}%", q)).unwrap_or_else(|| "-".into());
        lines.push(Line::from(vec![
            Span::styled("Signal:   ", Style::default().fg(Color::Gray)),
            Span::raw(format!("{} dBm ({})", signal, quality)),
        ]));
    }

    // Health breakdown
    if let Some(ref h) = iface.health {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Health breakdown:", Style::default().fg(Color::Yellow).bold()),
        ]));
        lines.push(Line::from(format!(
            "  RTT: {:.0}  Jitter: {:.0}  Loss: {:.0}  Throughput: {:.0}  Stability: {:.0}",
            h.rtt, h.jitter, h.loss, h.throughput, h.stability
        )));
    }

    let detail = Paragraph::new(lines);
    frame.render_widget(detail, inner);
}

/// Bonds tab — bond overview with member health.
fn render_bonds(frame: &mut Frame, app: &App, area: Rect) {
    if app.bonds.is_empty() {
        let lines = vec![
            Line::from("No bonds configured."),
            Line::from(""),
            Line::from("Add bonds to /etc/netfusion/netfusion.toml:"),
            Line::from(""),
            Line::from("  [[bonds]]"),
            Line::from("  name = \"netfusion0\""),
            Line::from("  mode = \"active_backup\""),
        ];
        let placeholder = Paragraph::new(lines)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(" Bond Manager "),
            area,
        );
        frame.render_widget(placeholder, area);
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Bond Manager ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    for bond in &app.bonds {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ({:?})", bond.name, bond.mode),
                Style::default().fg(Color::Cyan).bold(),
            ),
        ]));

        // Health gauge
        if let Some(ref h) = bond.health {
            let color = health_color(h.overall);
            lines.push(Line::from(format!(
                "  Health: [{:.0}/100]",
                h.overall
            )));
        }

        lines.push(Line::from(vec![
            Span::styled("Active: ", Style::default().fg(Color::Gray)),
            Span::raw(bond.active_members.join(", ")),
        ]));

        if !bond.standby_members.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Standby: ", Style::default().fg(Color::Gray)),
                Span::raw(bond.standby_members.join(", ")),
            ]));
        }

        if bond.failover_active {
            lines.push(Line::from(vec![
                Span::styled("  FAIL OVER ACTIVE", Style::default().fg(Color::Red).bold()),
            ]));
            if let Some(ref ts) = bond.last_failover {
                lines.push(Line::from(format!(
                    "  Last failover: {}",
                    ts.format("%Y-%m-%d %H:%M:%S")
                )));
            }
        }

        lines.push(Line::from(""));
    }

    let content = Paragraph::new(lines);
    frame.render_widget(content, inner);
}

/// Logs tab — color-coded event list with scrollbar.
fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    // Store viewport height
    let height = area.height.saturating_sub(2) as usize;

    let lines: Vec<Line> = app
        .events
        .iter()
        .skip(app.log_scroll_pos)
        .take(height + 5) // a few extra for safety
        .map(|e| {
            let desc = e.description();
            let timestamp = e.timestamp().format("%H:%M:%S");

            // Color by event type
            let color = if desc.contains("Up") || desc.contains("connected") {
                Color::Green
            } else if desc.contains("Down") || desc.contains("error") || desc.contains("Failover") {
                Color::Red
            } else if desc.contains("health") || desc.contains("Health") {
                Color::Yellow
            } else {
                Color::White
            };

            Line::from(vec![
                Span::styled(
                    format!("[{}] ", timestamp),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(desc, Style::default().fg(color)),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Event Log ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let logs = Paragraph::new(lines);
    frame.render_widget(logs, inner);

    // Scrollbar
    let mut scroll_state = ScrollbarState::default()
        .content_length(app.events.len())
        .position(app.log_scroll_pos);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        area,
        &mut scroll_state,
    );
}

/// Render the status bar with dynamic help text.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    // Dynamic help text based on active tab
    let help = match app.selected_tab {
        0 | 2 => "q:quit | ←/→:tabs | r:refresh",
        1 => "q:quit | ←/→:tabs | ↑↓/jk:select | r:refresh",
        3 => "q:quit | ←/→:tabs | jk/↑↓:scroll | g:top | G:bottom | PgUp/PgDn",
        _ => "q:quit | ←/→:tabs",
    };

    let refresh_info = app
        .last_refresh
        .map(|ts| format!("Last: {} | ", ts.format("%H:%M:%S")))
        .unwrap_or_default();

    let event_count = if !app.events.is_empty() {
        format!("Events: {} | ", app.events.len())
    } else {
        String::new()
    };

    let status_bar = Paragraph::new(format!("{}{}{}", refresh_info, event_count, help))
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    frame.render_widget(status_bar, area);
}
