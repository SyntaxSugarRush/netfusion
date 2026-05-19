// SPDX-License-Identifier: MIT OR Apache-2.0

//! NetFusion terminal user interface.
//!
//! A modern, keyboard-driven TUI for monitoring and managing
//! network aggregation, bonding, and routing.
//!
//! Architecture:
//! - Background data fetcher task polls the daemon every 5s
//! - Results sent via mpsc channel to the main loop
//! - Main loop renders at max 15fps, only when dirty

mod tui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use crate::tui::App;
use netfusion_daemon::ipc::client::IpcClient;

const DEFAULT_SOCKET_PATH: &str = "/run/netfusion/netfusion.sock";
const FETCH_INTERVAL_SECS: u64 = 5;
const RENDER_FPS: u64 = 15;

/// Update messages from the background fetcher to the main loop.
enum DataUpdate {
    Status(netfusion_shared::types::SystemStatus),
    Interfaces(Vec<netfusion_shared::types::InterfaceInfo>),
    Bonds(Vec<netfusion_shared::types::BondState>),
    Tunnels(Vec<netfusion_shared::types::TunnelState>),
    Loading(bool),
    Error(String),
}

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
    terminal.clear()?;

    // Create app state
    let mut app = App::new();

    // Try to connect to daemon
    let client = match IpcClient::connect(DEFAULT_SOCKET_PATH).await {
        Ok(c) => {
            app.connected = true;
            Some(c)
        }
        Err(e) => {
            app.error = Some(format!("Daemon not running: {}", e));
            None
        }
    };

    // Channel for background fetcher → main loop
    let (data_tx, mut data_rx) = mpsc::channel::<DataUpdate>(4);

    // Spawn background data fetcher
    if let Some(client) = client {
        tokio::spawn(data_fetcher_loop(client, data_tx));
    }

    // Render tick — capped at 15fps
    let mut render_tick = interval(Duration::from_millis(1000 / RENDER_FPS));
    render_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    // Initial data fetch trigger
    app.set_loading(true);

    // Main loop
    while app.running {
        tokio::select! {
            // Data update from background fetcher
            Some(update) = data_rx.recv() => {
                match update {
                    DataUpdate::Status(s) => app.apply_status(s),
                    DataUpdate::Interfaces(i) => app.apply_interfaces(i),
                    DataUpdate::Bonds(b) => app.apply_bonds(b),
                    DataUpdate::Tunnels(t) => app.apply_tunnels(t),
                    DataUpdate::Loading(loading) => {
                        app.set_loading(loading);
                        if !loading {
                            app.set_last_refresh();
                        }
                    }
                    DataUpdate::Error(e) => app.set_error(e),
                }
            }

            // Keyboard input
            maybe_event = poll_input() => {
                if let Some(Ok(evt)) = maybe_event {
                    handle_key(&mut app, evt);
                }
            }

            // Render tick (15fps cap)
            _ = render_tick.tick() => {
                if app.take_dirty() {
                    if let Err(e) = terminal.draw(|frame| crate::tui::ui::render(frame, &app)) {
                        warn!("Render error: {}", e);
                    }
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

/// Poll for keyboard input without blocking the async runtime.
async fn poll_input() -> Option<std::io::Result<Event>> {
    tokio::task::spawn_blocking(|| {
        if event::poll(Duration::from_millis(100)).ok()? {
            Some(event::read())
        } else {
            None
        }
    })
    .await
    .ok()
    .flatten()
}

/// Handle keyboard input.
fn handle_key(app: &mut App, event: Event) {
    if let Event::Key(key) = event {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return;
        }

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
                // Manual refresh — signal is implicit via the background loop
                app.error = None;
            }
            // Tab-specific navigation
            _ => match app.selected_tab {
                1 => {
                    // Interfaces tab: arrow keys / jk for selection
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.select_interface_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.select_interface_down(),
                        _ => {}
                    }
                }
                3 => {
                    // Logs tab: jk/arrows for scroll, g/G for top/bottom
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.scroll_logs_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.scroll_logs_down(),
                        KeyCode::PageUp => app.scroll_logs_page_up(),
                        KeyCode::PageDown => app.scroll_logs_page_down(),
                        KeyCode::Char('g') => {
                            if key.modifiers.contains(KeyModifiers::CONTROL) {
                                app.scroll_logs_bottom();
                            } else {
                                app.scroll_logs_top();
                            }
                        }
                        KeyCode::Char('G') => app.scroll_logs_bottom(),
                        _ => {}
                    }
                }
                _ => {}
            },
        }
    }
}

/// Background task that periodically fetches data from the daemon.
async fn data_fetcher_loop(mut client: IpcClient, tx: mpsc::Sender<DataUpdate>) {
    let mut interval = interval(Duration::from_secs(FETCH_INTERVAL_SECS));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        interval.tick().await;

        tx.send(DataUpdate::Loading(true)).await.ok();

        // Sequential fetchs (single UnixStream — cannot parallelize)
        match client.get_status().await {
            Ok(status) => {
                let _ = tx.send(DataUpdate::Status(status)).await;
            }
            Err(e) => {
                let _ = tx.send(DataUpdate::Error(format!("Status fetch failed: {}", e))).await;
            }
        }

        match client.get_interfaces().await {
            Ok(interfaces) => {
                let _ = tx.send(DataUpdate::Interfaces(interfaces)).await;
            }
            Err(e) => {
                let _ = tx.send(DataUpdate::Error(format!("Interface fetch failed: {}", e))).await;
            }
        }

        match client.get_bonds().await {
            Ok(bonds) => {
                let _ = tx.send(DataUpdate::Bonds(bonds)).await;
            }
            Err(_) => {} // Bonds may not be configured — silent
        }

        match client.get_tunnels().await {
            Ok(tunnels) => {
                let _ = tx.send(DataUpdate::Tunnels(tunnels)).await;
            }
            Err(_) => {} // Tunnels may not be configured — silent
        }

        tx.send(DataUpdate::Loading(false)).await.ok();
    }
}
