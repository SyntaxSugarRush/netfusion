// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tunnel manager — orchestrates multiple tunnels with health monitoring
//! and auto-reconnect.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use netfusion_shared::config::{TunnelConfig, TunnelType};
use netfusion_shared::types::TunnelState;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::error::CoreError;
use crate::tunnels::wireguard::{WireGuardConfig, WireGuardTunnel};

/// How often to check tunnel health.
const HEALTH_CHECK_INTERVAL_SECS: u64 = 10;

/// Maximum reconnect delay.
const MAX_RECONNECT_DELAY_SECS: u64 = 300;

/// Manages all active tunnels.
pub struct TunnelManager {
    /// Active WireGuard tunnels.
    wg_tunnels: HashMap<String, WireGuardTunnel>,

    /// Tunnel configurations.
    configs: HashMap<String, TunnelConfig>,

    /// Reconnect state per tunnel.
    reconnect_state: HashMap<String, ReconnectInfo>,

    /// Tunnel health scores (updated on each check).
    health_scores: HashMap<String, f64>,
}

/// Tracks reconnection state for a tunnel.
struct ReconnectInfo {
    /// Number of consecutive failed reconnect attempts.
    attempts: u32,

    /// Time of last reconnect attempt.
    last_attempt: Option<Instant>,

    /// Current reconnect delay.
    current_delay: Duration,
}

impl ReconnectInfo {
    fn new() -> Self {
        Self {
            attempts: 0,
            last_attempt: None,
            current_delay: Duration::from_secs(1),
        }
    }

    /// Calculate the next reconnect delay with exponential backoff.
    fn next_delay(&mut self) -> Duration {
        self.attempts += 1;
        self.current_delay = std::cmp::min(
            self.current_delay * 2,
            Duration::from_secs(MAX_RECONNECT_DELAY_SECS),
        );
        self.current_delay
    }

    /// Reset the reconnect state (after successful connection).
    fn reset(&mut self) {
        self.attempts = 0;
        self.current_delay = Duration::from_secs(1);
        self.last_attempt = Some(Instant::now());
    }

    /// Check if we should attempt to reconnect now.
    fn should_reconnect(&self) -> bool {
        match self.last_attempt {
            Some(last) => last.elapsed() >= self.current_delay,
            None => true,
        }
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            wg_tunnels: HashMap::new(),
            configs: HashMap::new(),
            reconnect_state: HashMap::new(),
            health_scores: HashMap::new(),
        }
    }

    /// Add a tunnel configuration.
    pub fn add_config(&mut self, config: TunnelConfig) {
        info!(name = %config.name, r#type = ?config.r#type, "Tunnel configuration added");
        self.configs.insert(config.name.clone(), config);
    }

    /// Remove a tunnel.
    pub async fn remove_tunnel(&mut self, name: &str) -> Result<(), CoreError> {
        if let Some(mut tunnel) = self.wg_tunnels.remove(name) {
            tunnel.teardown()?;
        }
        self.configs.remove(name);
        self.reconnect_state.remove(name);
        self.health_scores.remove(name);
        info!(name, "Tunnel removed");
        Ok(())
    }

    /// Start all configured tunnels that have auto_connect enabled.
    pub async fn start_auto_connect(&mut self) {
        let auto_connect: Vec<_> = self
            .configs
            .values()
            .filter(|c| c.auto_connect)
            .cloned()
            .collect();

        for config in auto_connect {
            if let Err(e) = self.start_tunnel(&config).await {
                warn!(name = %config.name, error = %e, "Failed to start tunnel");
            }
        }
    }

    /// Start a single tunnel.
    async fn start_tunnel(&mut self, config: &TunnelConfig) -> Result<(), CoreError> {
        match config.r#type {
            TunnelType::WireGuard => self.start_wireguard(config).await?,
            TunnelType::Quic => {
                warn!(name = %config.name, "QUIC tunnel not yet implemented");
            }
            TunnelType::Relay => {
                warn!(name = %config.name, "Relay tunnel not yet implemented");
            }
            TunnelType::OpenVpn => {
                warn!(name = %config.name, "OpenVPN tunnel not supported");
            }
            TunnelType::Tailscale => {
                warn!(name = %config.name, "Tailscale tunnel not supported");
            }
        }
        Ok(())
    }

    /// Start a WireGuard tunnel from configuration.
    async fn start_wireguard(&mut self, config: &TunnelConfig) -> Result<(), CoreError> {
        // Extract WireGuard-specific options
        let private_key = config
            .options
            .get("private_key")
            .ok_or_else(|| CoreError::Qos(format!("Missing private_key for tunnel {}", config.name)))?
            .clone();

        let public_key = config
            .options
            .get("public_key")
            .ok_or_else(|| CoreError::Qos(format!("Missing public_key for tunnel {}", config.name)))?
            .clone();

        let tunnel_address = config
            .options
            .get("tunnel_address")
            .ok_or_else(|| CoreError::Qos(format!("Missing tunnel_address for tunnel {}", config.name)))?
            .clone();

        let allowed_ips: Vec<String> = config
            .options
            .get("allowed_ips")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["0.0.0.0/0".into()]);

        let listen_port = config
            .options
            .get("listen_port")
            .and_then(|s| s.parse::<u16>().ok());

        let persistent_keepalive = config
            .options
            .get("persistent_keepalive")
            .and_then(|s| s.parse::<u16>().ok());

        let dns_servers: Vec<String> = config
            .options
            .get("dns_servers")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();

        let preshared_key = config.options.get("preshared_key").cloned();

        let wg_config = WireGuardConfig {
            interface: config.name.clone(),
            private_key,
            listen_port,
            endpoint: config.remote.clone(),
            public_key,
            preshared_key,
            allowed_ips,
            persistent_keepalive,
            tunnel_address,
            dns_servers,
        };

        let mut tunnel = WireGuardTunnel::new(wg_config);
        tunnel.create()?;

        self.wg_tunnels.insert(config.name.clone(), tunnel);
        self.reconnect_state.insert(config.name.clone(), ReconnectInfo::new());

        Ok(())
    }

    /// Check health of all active tunnels.
    pub fn check_health(&mut self) {
        let unhealthy: Vec<String> = self
            .wg_tunnels
            .keys()
            .filter_map(|name| {
                let tunnel = self.wg_tunnels.get(name).unwrap();
                if tunnel.is_operational() {
                    match tunnel.check_handshake() {
                        Ok(Some(_last_hs)) => {
                            // Handshake exists — check via transfer stats instead
                            // For now, mark as healthy if operational
                            self.health_scores.insert(name.clone(), 100.0);
                            None
                        }
                        Ok(None) => {
                            // No handshake ever
                            self.health_scores.insert(name.clone(), 0.0);
                            Some(name.clone())
                        }
                        Err(_) => {
                            self.health_scores.insert(name.clone(), 0.0);
                            Some(name.clone())
                        }
                    }
                } else {
                    self.health_scores.insert(name.clone(), 0.0);
                    Some(name.clone())
                }
            })
            .collect();

        // Attempt reconnect for unhealthy tunnels
        for name in unhealthy {
            self.try_reconnect(&name);
        }
    }

    /// Attempt to reconnect a failed tunnel.
    fn try_reconnect(&mut self, name: &str) {
        let reconnect = self.reconnect_state.entry(name.to_string()).or_insert_with(ReconnectInfo::new);

        if !reconnect.should_reconnect() {
            debug!(name, "Reconnect cooldown active");
            return;
        }

        // Only attempt if auto_reconnect is enabled
        let auto_reconnect = self
            .configs
            .get(name)
            .map(|c| c.auto_reconnect)
            .unwrap_or(false);

        if !auto_reconnect {
            return;
        }

        reconnect.last_attempt = Some(Instant::now());
        let delay = reconnect.next_delay();

        info!(name, attempt = reconnect.attempts, delay = ?delay, "Attempting tunnel reconnect");

        // Try to recreate the tunnel — clone config to avoid borrow conflict
        let config = self.configs.get(name).cloned();
        if let Some(config) = config {
            // First tear down existing
            if let Some(mut tunnel) = self.wg_tunnels.remove(name) {
                let _ = tunnel.teardown();
            }

            // Then try to recreate
            match self.start_tunnel_blocking(&config) {
                Ok(()) => {
                    info!(name, "Tunnel reconnected successfully");
                    if let Some(reconnect) = self.reconnect_state.get_mut(name) {
                        reconnect.reset();
                    }
                }
                Err(e) => {
                    warn!(name, error = %e, "Tunnel reconnect failed");
                }
            }
        }
    }

    /// Blocking version of start_tunnel for use outside async context.
    fn start_tunnel_blocking(&mut self, config: &TunnelConfig) -> Result<(), CoreError> {
        match config.r#type {
            TunnelType::WireGuard => {
                // Inline the WireGuard startup logic
                let private_key = config
                    .options
                    .get("private_key")
                    .ok_or_else(|| CoreError::Qos(format!("Missing private_key for tunnel {}", config.name)))?
                    .clone();

                let public_key = config
                    .options
                    .get("public_key")
                    .ok_or_else(|| CoreError::Qos(format!("Missing public_key for tunnel {}", config.name)))?
                    .clone();

                let tunnel_address = config
                    .options
                    .get("tunnel_address")
                    .ok_or_else(|| CoreError::Qos(format!("Missing tunnel_address for tunnel {}", config.name)))?
                    .clone();

                let allowed_ips: Vec<String> = config
                    .options
                    .get("allowed_ips")
                    .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_else(|| vec!["0.0.0.0/0".into()]);

                let listen_port = config
                    .options
                    .get("listen_port")
                    .and_then(|s| s.parse::<u16>().ok());

                let persistent_keepalive = config
                    .options
                    .get("persistent_keepalive")
                    .and_then(|s| s.parse::<u16>().ok());

                let dns_servers: Vec<String> = config
                    .options
                    .get("dns_servers")
                    .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default();

                let preshared_key = config.options.get("preshared_key").cloned();

                let wg_config = WireGuardConfig {
                    interface: config.name.clone(),
                    private_key,
                    listen_port,
                    endpoint: config.remote.clone(),
                    public_key,
                    preshared_key,
                    allowed_ips,
                    persistent_keepalive,
                    tunnel_address,
                    dns_servers,
                };

                let mut tunnel = WireGuardTunnel::new(wg_config);
                tunnel.create()?;

                self.wg_tunnels.insert(config.name.clone(), tunnel);
                Ok(())
            }
            _ => Err(CoreError::Qos(format!("Tunnel type {:?} not supported for reconnect", config.r#type))),
        }
    }

    /// Get the current state of all tunnels.
    pub fn get_states(&self) -> Vec<TunnelState> {
        let mut states = Vec::new();

        for (name, config) in &self.configs {
            let tunnel = self.wg_tunnels.get(name);
            let connected = tunnel.map(|t| t.is_operational()).unwrap_or(false);

            let (rx_bytes, tx_bytes) = tunnel
                .and_then(|t| t.get_transfer_bytes().ok())
                .unwrap_or((0, 0));

            let reconnect_count = self
                .reconnect_state
                .get(name)
                .map(|r| r.attempts)
                .unwrap_or(0);

            let connected_since = if connected {
                Some(chrono::Utc::now())
            } else {
                None
            };

            states.push(TunnelState {
                name: name.clone(),
                connected,
                remote: config.remote.clone(),
                interface: tunnel.map(|_| name.clone()),
                connected_since,
                tx_bytes,
                rx_bytes,
                reconnect_count,
                last_error: None,
            });
        }

        states
    }

    /// Get health score for a specific tunnel.
    pub fn get_health(&self, name: &str) -> Option<f64> {
        self.health_scores.get(name).copied()
    }

    /// Get all tunnel health scores.
    pub fn all_health_scores(&self) -> &HashMap<String, f64> {
        &self.health_scores
    }

    /// Run the tunnel health check loop.
    pub async fn run_health_loop(mut self) {
        info!("Tunnel health monitor starting");

        let mut interval = interval(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS));

        loop {
            interval.tick().await;
            self.check_health();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconnect_backoff() {
        let mut info = ReconnectInfo::new();

        // First attempt — should always be allowed (last_attempt is None)
        assert!(info.should_reconnect());

        // Simulate an attempt
        info.next_delay();
        info.last_attempt = Some(Instant::now());
        assert!(!info.should_reconnect()); // cooldown active

        // After delay expires...
        info.last_attempt = Some(Instant::now() - info.current_delay - Duration::from_secs(1));
        assert!(info.should_reconnect());

        info.next_delay();
        let prev_delay = info.current_delay;
        info.next_delay();
        assert!(info.current_delay > prev_delay); // exponential growth
    }

    #[test]
    fn test_reconnect_max_delay() {
        let mut info = ReconnectInfo::new();

        // Simulate many reconnect attempts
        for _ in 0..20 {
            info.next_delay();
        }

        assert!(info.current_delay <= Duration::from_secs(MAX_RECONNECT_DELAY_SECS));
    }

    #[test]
    fn test_reconnect_reset() {
        let mut info = ReconnectInfo::new();
        info.next_delay();
        info.next_delay();
        assert!(info.attempts > 0);

        info.reset();
        assert_eq!(info.attempts, 0);
        assert_eq!(info.current_delay, Duration::from_secs(1));
    }

    #[test]
    fn test_tunnel_manager_empty() {
        let manager = TunnelManager::new();
        assert!(manager.get_states().is_empty());
        assert!(manager.all_health_scores().is_empty());
    }

    #[test]
    fn test_tunnel_config_added() {
        let mut manager = TunnelManager::new();
        manager.add_config(TunnelConfig {
            name: "test_tunnel".into(),
            r#type: TunnelType::WireGuard,
            remote: "example.com:51820".into(),
            local_bind: None,
            auth_ref: None,
            options: HashMap::new(),
            auto_connect: false,
            auto_reconnect: true,
            reconnect_interval_secs: 30,
        });
        assert_eq!(manager.configs.len(), 1);
    }
}
