// SPDX-License-Identifier: MIT OR Apache-2.0

//! NetFusion terminal user interface.
//!
//! A modern, keyboard-driven TUI for monitoring and managing
//! network aggregation, bonding, and routing.

mod tui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use tokio::time::interval;
use tracing::info;

use crate::tui::App;

const DEFAULT_SOCKET_PATH: &str = "/run/netfusion/netfusion.sock";
const REFRESH_INTERVAL_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "netfusion=info".into()),
        )
        .init();

    info!("NetFusion TUI starting");

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new();

    // Try to connect to daemon
    let mut client = match netfusion_daemon::ipc::client::IpcClient::connect(DEFAULT_SOCKET_PATH).await {
        Ok(c) => {
            app.connected = true;
            Some(c)
        }
        Err(e) => {
            app.error = Some(format!("Daemon not running: {}", e));
            None
        }
    };

    // Initial data fetch
    if let Some(ref mut c) = client {
        if let Ok(interfaces) = c.get_interfaces().await {
            app.interfaces = interfaces;
        }
        if let Ok(status) = c.get_status().await {
            app.status = Some(status);
        }
    }

    // Main loop
    let mut refresh = interval(Duration::from_secs(REFRESH_INTERVAL_SECS));

    while app.running {
        // Render
        terminal.draw(|frame| crate::tui::ui::render(frame, &app))?;

        // Poll for input with timeout
        if crossterm::event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                handle_key(&mut app, &mut client, key).await;
            }
        }

        // Periodic refresh
        refresh.tick().await;
        if app.connected {
            if let Some(ref mut c) = client {
                if let Ok(interfaces) = c.get_interfaces().await {
                    app.interfaces = interfaces;
                }
                if let Ok(status) = c.get_status().await {
                    app.status = Some(status);
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    info!("NetFusion TUI exiting");
    Ok(())
}

/// Handle keyboard input.
async fn handle_key(
    app: &mut App,
    client: &mut Option<netfusion_daemon::ipc::client::IpcClient>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Char('q') => {
            app.running = false;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.next_tab();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.prev_tab();
        }
        KeyCode::Char('r') => {
            // Manual refresh
            app.error = None;
            if let Some(c) = client {
                if let Ok(interfaces) = c.get_interfaces().await {
                    app.interfaces = interfaces;
                }
                if let Ok(status) = c.get_status().await {
                    app.status = Some(status);
                }
            }
        }
        _ => {}
    }
}
