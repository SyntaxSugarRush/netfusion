// SPDX-License-Identifier: MIT OR Apache-2.0

//! UI rendering for the TUI.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, Tabs};

use crate::tui::App;

/// Tab names.
const TAB_NAMES: &[&str] = &["Dashboard", "Interfaces", "Bonds", "Logs"];

/// Render the full UI.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header + tabs
            Constraint::Min(0),    // Content
            Constraint::Length(3), // Status bar
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

    // Title bar
    let title = Paragraph::new(" NetFusion ")
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

/// Dashboard view.
fn render_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    // Connection status
    let conn_text = if app.connected {
        "● Connected"
    } else {
        "○ Disconnected"
    };
    let conn_color = if app.connected { Color::Green } else { Color::Red };
    let conn = Paragraph::new(conn_text).style(Style::default().fg(conn_color));
    frame.render_widget(
        Block::default().borders(Borders::ALL).title(" Status "),
        chunks[0],
    );
    frame.render_widget(conn, chunks[0]);

    // System status
    if let Some(ref status) = app.status {
        let status_text = format!(
            "Interfaces: {}  |  Bonds: {}  |  Tunnels: {}  |  Uptime: {}s",
            status.total_interfaces,
            status.active_bonds,
            status.connected_tunnels,
            status.uptime_secs,
        );
        let status_para = Paragraph::new(status_text);
        frame.render_widget(
            Block::default().borders(Borders::ALL).title(" System "),
            chunks[1],
        );
        frame.render_widget(status_para, chunks[1]);
    }

    // Quick interface overview
    let rows: Vec<Row> = app
        .interfaces
        .iter()
        .map(|iface| {
            let state = match iface.link_state {
                netfusion_shared::types::LinkState::Up => "UP",
                netfusion_shared::types::LinkState::Down => "DOWN",
                netfusion_shared::types::LinkState::Unknown => "???",
            };
            let health = iface
                .health
                .as_ref()
                .map(|h| format!("{:.0}", h.overall))
                .unwrap_or_else(|| "N/A".into());
            Row::new(vec![
                iface.name.clone(),
                format!("{:?}", iface.if_type),
                state.into(),
                health,
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(6),
        ],
    )
    .header(
        Row::new(vec!["Interface", "Type", "State", "Health"])
            .style(Style::default().fg(Color::Yellow)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Interfaces "));
    frame.render_widget(table, chunks[2]);
}

/// Interfaces view.
fn render_interfaces(frame: &mut Frame, app: &App, area: Rect) {
    let rows: Vec<Row> = app
        .interfaces
        .iter()
        .map(|iface| {
            let addr = iface
                .addresses
                .first()
                .map(|a| a.cidr.clone())
                .unwrap_or_default();
            let gw = iface.gateway.as_deref().unwrap_or("-");
            let speed = iface
                .speed_mbps
                .map(|s| format!("{} Mbps", s))
                .unwrap_or_default();
            let mac = iface.mac.as_deref().unwrap_or("-");
            Row::new(vec![
                iface.name.clone(),
                format!("{:?}", iface.if_type),
                addr,
                gw.into(),
                speed,
                mac.into(),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(20),
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Length(18),
        ],
    )
    .header(
        Row::new(vec!["Interface", "Type", "Address", "Gateway", "Speed", "MAC"])
            .style(Style::default().fg(Color::Yellow)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Interface Details "));
    frame.render_widget(table, area);
}

/// Bonds view (placeholder).
fn render_bonds(frame: &mut Frame, _app: &App, area: Rect) {
    let placeholder = Paragraph::new("Bond management coming soon...")
        .alignment(Alignment::Center);
    frame.render_widget(
        Block::default().borders(Borders::ALL).title(" Bond Manager "),
        area,
    );
    frame.render_widget(placeholder, area);
}

/// Logs view.
fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app
        .events
        .iter()
        .map(|e| {
            let desc = e.description();
            let timestamp = e.timestamp().format("%H:%M:%S");
            Line::from(format!("[{}] {}", timestamp, desc))
        })
        .collect();

    let logs = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Event Log "),
    );
    frame.render_widget(logs, area);
}

/// Render the status bar.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let help_text = " q: quit | ←/→: tabs | r: refresh ";

    let error_text = app
        .error
        .as_ref()
        .map(|e| format!(" Error: {} ", e))
        .unwrap_or_default();

    let status_bar = Paragraph::new(format!("{}{}", error_text, help_text))
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    frame.render_widget(status_bar, area);
}
