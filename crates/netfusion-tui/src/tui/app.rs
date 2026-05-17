// SPDX-License-Identifier: MIT OR Apache-2.0

//! Application state for the TUI.

use netfusion_shared::types::{InterfaceInfo, SystemStatus};
use netfusion_shared::events::NetfusionEvent;
use tokio::sync::broadcast;

/// The main application state.
pub struct App {
    /// Whether the app should keep running.
    pub running: bool,

    /// Current system status.
    pub status: Option<SystemStatus>,

    /// List of discovered interfaces.
    pub interfaces: Vec<InterfaceInfo>,

    /// Recent events (rolling buffer).
    pub events: Vec<NetfusionEvent>,

    /// Currently selected tab (0=dashboard, 1=interfaces, 2=bonds, 3=logs).
    pub selected_tab: usize,

    /// Whether we're connected to the daemon.
    pub connected: bool,

    /// Last error message.
    pub error: Option<String>,

    /// Event subscription receiver (if subscribed).
    pub event_rx: Option<broadcast::Receiver<NetfusionEvent>>,
}

impl App {
    /// Create a new app with default state.
    pub fn new() -> Self {
        Self {
            running: true,
            status: None,
            interfaces: Vec::new(),
            events: Vec::new(),
            selected_tab: 0,
            connected: false,
            error: None,
            event_rx: None,
        }
    }

    /// Move to the next tab.
    pub fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % 4;
    }

    /// Move to the previous tab.
    pub fn prev_tab(&mut self) {
        self.selected_tab = if self.selected_tab == 0 {
            3
        } else {
            self.selected_tab - 1
        };
    }

    /// Add an event to the rolling buffer (max 100).
    pub fn push_event(&mut self, event: NetfusionEvent) {
        self.events.push(event);
        if self.events.len() > 100 {
            self.events.remove(0);
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
