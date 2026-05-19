// SPDX-License-Identifier: MIT OR Apache-2.0

//! Application state for the TUI.

use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use netfusion_shared::types::{BondState, InterfaceInfo, SystemStatus, TunnelState};
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
    pub events: VecDeque<NetfusionEvent>,

    /// Currently selected tab (0=dashboard, 1=interfaces, 2=bonds, 3=logs).
    pub selected_tab: usize,

    /// Whether we're connected to the daemon.
    pub connected: bool,

    /// Last error message.
    pub error: Option<String>,

    /// Event subscription receiver (if subscribed).
    pub event_rx: Option<broadcast::Receiver<NetfusionEvent>>,

    // --- New fields ---

    /// Set when data changes; consumed by the render loop.
    pub dirty: bool,

    /// True while a data fetch is in flight.
    pub is_loading: bool,

    /// Timestamp of last successful data fetch.
    pub last_refresh: Option<DateTime<Utc>>,

    /// Rolling health score history (max 60 samples) for sparkline.
    pub health_history: VecDeque<f64>,

    /// Bond states (fetched from daemon).
    pub bonds: Vec<BondState>,

    /// Tunnel states (fetched from daemon).
    pub tunnels: Vec<TunnelState>,

    /// Scroll position for the interfaces table.
    pub interface_scroll_pos: usize,

    /// Scroll position for the logs list.
    pub log_scroll_pos: usize,

    /// Cursor position within the interfaces list.
    pub selected_interface_idx: usize,

    /// Viewport height for the interfaces table (set during render).
    pub interface_view_height: u16,

    /// Viewport height for the logs area (set during render).
    pub log_view_height: u16,
}

impl App {
    /// Create a new app with default state.
    pub fn new() -> Self {
        Self {
            running: true,
            status: None,
            interfaces: Vec::new(),
            events: VecDeque::with_capacity(100),
            selected_tab: 0,
            connected: false,
            error: None,
            event_rx: None,
            dirty: false,
            is_loading: false,
            last_refresh: None,
            health_history: VecDeque::with_capacity(60),
            bonds: Vec::new(),
            tunnels: Vec::new(),
            interface_scroll_pos: 0,
            log_scroll_pos: 0,
            selected_interface_idx: 0,
            interface_view_height: 0,
            log_view_height: 0,
        }
    }

    /// Move to the next tab.
    pub fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % 4;
        self.mark_dirty();
    }

    /// Move to the previous tab.
    pub fn prev_tab(&mut self) {
        self.selected_tab = if self.selected_tab == 0 {
            3
        } else {
            self.selected_tab - 1
        };
        self.mark_dirty();
    }

    /// Add an event to the rolling buffer (max 100).
    pub fn push_event(&mut self, event: NetfusionEvent) {
        self.events.push_back(event);
        while self.events.len() > 100 {
            self.events.pop_front();
        }
    }

    /// Mark the app as needing a re-render.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Consume the dirty flag — returns true if a re-render is needed.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Push a health score sample into the rolling history (max 60).
    pub fn push_health_sample(&mut self, score: f64) {
        self.health_history.push_back(score);
        while self.health_history.len() > 60 {
            self.health_history.pop_front();
        }
    }

    /// Navigate interface selection up.
    pub fn select_interface_up(&mut self) {
        if self.selected_interface_idx > 0 {
            self.selected_interface_idx -= 1;
            self.mark_dirty();
        }
    }

    /// Navigate interface selection down.
    pub fn select_interface_down(&mut self) {
        if self.selected_interface_idx + 1 < self.interfaces.len() {
            self.selected_interface_idx += 1;
            self.mark_dirty();
        }
    }

    /// Scroll logs up.
    pub fn scroll_logs_up(&mut self) {
        self.log_scroll_pos = self.log_scroll_pos.saturating_sub(3);
        self.mark_dirty();
    }

    /// Scroll logs down.
    pub fn scroll_logs_down(&mut self) {
        let max = self.events.len().saturating_sub(self.log_view_height as usize);
        self.log_scroll_pos = (self.log_scroll_pos + 3).min(max);
        self.mark_dirty();
    }

    /// Page up in logs.
    pub fn scroll_logs_page_up(&mut self) {
        let step = self.log_view_height as usize;
        self.log_scroll_pos = self.log_scroll_pos.saturating_sub(step);
        self.mark_dirty();
    }

    /// Page down in logs.
    pub fn scroll_logs_page_down(&mut self) {
        let max = self.events.len().saturating_sub(self.log_view_height as usize);
        let step = self.log_view_height as usize;
        self.log_scroll_pos = (self.log_scroll_pos + step).min(max);
        self.mark_dirty();
    }

    /// Jump logs to top.
    pub fn scroll_logs_top(&mut self) {
        self.log_scroll_pos = 0;
        self.mark_dirty();
    }

    /// Jump logs to bottom.
    pub fn scroll_logs_bottom(&mut self) {
        let max = self.events.len().saturating_sub(self.log_view_height as usize);
        self.log_scroll_pos = max;
        self.mark_dirty();
    }

    /// Apply a batch of events from the background fetcher.
    pub fn apply_status(&mut self, status: SystemStatus) {
        if let Some(ref h) = status.health {
            self.push_health_sample(h.overall);
        }
        self.status = Some(status);
        self.mark_dirty();
    }

    pub fn apply_interfaces(&mut self, interfaces: Vec<InterfaceInfo>) {
        // Collect health samples before replacing
        let health_samples: Vec<f64> = interfaces
            .iter()
            .filter_map(|iface| iface.health.as_ref().map(|h| h.overall))
            .collect();

        self.interfaces = interfaces;
        if self.selected_interface_idx >= self.interfaces.len() {
            self.selected_interface_idx = self.interfaces.len().saturating_sub(1);
        }

        for score in health_samples {
            self.push_health_sample(score);
        }
        self.mark_dirty();
    }

    pub fn apply_bonds(&mut self, bonds: Vec<BondState>) {
        self.bonds = bonds;
        self.mark_dirty();
    }

    pub fn apply_tunnels(&mut self, tunnels: Vec<TunnelState>) {
        self.tunnels = tunnels;
        self.mark_dirty();
    }

    pub fn apply_events(&mut self, events: Vec<NetfusionEvent>) {
        for e in events {
            self.push_event(e);
        }
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.is_loading = loading;
        self.mark_dirty();
    }

    pub fn set_error(&mut self, err: String) {
        self.error = Some(err);
        self.mark_dirty();
    }

    pub fn set_last_refresh(&mut self) {
        self.last_refresh = Some(Utc::now());
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
