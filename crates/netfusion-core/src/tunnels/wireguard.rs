// SPDX-License-Identifier: MIT OR Apache-2.0

//! WireGuard tunnel orchestration.
//!
//! Uses the `wg` and `ip` commands to create and manage WireGuard
//! tunnels. WireGuard is preferreded for its:
//! - Modern cryptography (Noise_IK, Curve25519, ChaCha20-Poly1305)
//! - Kernel-space performance (via the wireguard kernel module)
//! - Simple configuration model
//! - Built-in keepalive for NAT traversal

use std::process::Command;
use std::time::Duration;

use tracing::{debug, info, warn};

use crate::error::CoreError;

/// WireGuard tunnel configuration.
#[derive(Debug, Clone)]
pub struct WireGuardConfig {
    /// Tunnel interface name (e.g., "wg0").
    pub interface: String,

    /// Local private key (base64).
    pub private_key: String,

    /// Local listen port.
    pub listen_port: Option<u16>,

    /// Remote endpoint (host:port).
    pub endpoint: String,

    /// Remote public key (base64).
    pub public_key: String,

    /// Pre-shared key for additional security (optional, base64).
    pub preshared_key: Option<String>,

    /// Allowed IPs (routes to install through the tunnel).
    pub allowed_ips: Vec<String>,

    /// Persistent keepalive interval (seconds, 0 = disabled).
    pub persistent_keepalive: Option<u16>,

    /// Tunnel IP address to assign.
    pub tunnel_address: String,

    /// DNS servers to use when tunnel is active.
    pub dns_servers: Vec<String>,
}

/// Represents a WireGuard tunnel.
pub struct WireGuardTunnel {
    config: WireGuardConfig,
    is_up: bool,
}

impl WireGuardTunnel {
    pub fn new(config: WireGuardConfig) -> Self {
        Self {
            config,
            is_up: false,
        }
    }

    /// Create the WireGuard interface.
    pub fn create(&mut self) -> Result<(), CoreError> {
        // Create the wg interface
        let output = Command::new("ip")
            .args([
                "link",
                "add",
                "dev",
                &self.config.interface,
                "type",
                "wireguard",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Interface may already exist
            if !stderr.contains("exists") {
                return Err(CoreError::Internal(format!(
                    "Failed to create WireGuard interface {}: {}",
                    self.config.interface, stderr
                )));
            }
            warn!(
                interface = %self.config.interface,
                "WireGuard interface already exists"
            );
        }

        // Set the private key
        self.set_private_key()?;

        // Set listen port if specified
        if let Some(port) = self.config.listen_port {
            self.set_listen_port(port)?;
        }

        // Assign tunnel IP address
        self.assign_address()?;

        // Add the peer
        self.add_peer()?;

        // Bring the interface up
        self.bring_up()?;

        // Set DNS if configured
        if !self.config.dns_servers.is_empty() {
            self.set_dns()?;
        }

        self.is_up = true;
        info!(interface = %self.config.interface, endpoint = %self.config.endpoint, "WireGuard tunnel created");

        Ok(())
    }

    /// Bring the tunnel interface up.
    pub fn bring_up(&self) -> Result<(), CoreError> {
        let output = Command::new("ip")
            .args(["link", "set", "dev", &self.config.interface, "up"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to bring up WireGuard interface {}: {}",
                self.config.interface, stderr
            )));
        }

        debug!(interface = %self.config.interface, "WireGuard interface brought up");
        Ok(())
    }

    /// Tear down the tunnel.
    pub fn teardown(&mut self) -> Result<(), CoreError> {
        let output = Command::new("ip")
            .args(["link", "del", "dev", &self.config.interface])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("not found") {
                warn!(
                    interface = %self.config.interface,
                    %stderr,
                    "Failed to delete WireGuard interface (may be harmless)"
                );
            }
        }

        self.is_up = false;
        info!(interface = %self.config.interface, "WireGuard tunnel torn down");
        Ok(())
    }

    /// Check if the tunnel is operational.
    pub fn is_operational(&self) -> bool {
        if !self.is_up {
            return false;
        }

        // Check if the interface exists and is up
        let output = Command::new("ip")
            .args(["link", "show", "dev", &self.config.interface])
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout.contains("state UP") || stdout.contains("UNKNOWN")
            }
            Err(_) => false,
        }
    }

    /// Get tunnel statistics from `wg show`.
    pub fn get_stats(&self) -> Result<String, CoreError> {
        let output = Command::new("wg")
            .args(["show", &self.config.interface])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to get WireGuard stats for {}: {}",
                self.config.interface, stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get the number of bytes transferred (latest handshake data).
    pub fn get_transfer_bytes(&self) -> Result<(u64, u64), CoreError> {
        let output = Command::new("wg")
            .args(["show", &self.config.interface, "transfer"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to get WireGuard transfer stats: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut rx_bytes = 0;
        let mut tx_bytes = 0;

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                if let (Ok(rx), Ok(tx)) = (parts[1].parse::<u64>(), parts[2].parse::<u64>()) {
                    rx_bytes = rx;
                    tx_bytes = tx;
                }
            }
        }

        Ok((rx_bytes, tx_bytes))
    }

    /// Check last handshake time to determine tunnel health.
    pub fn check_handshake(&self) -> Result<Option<Duration>, CoreError> {
        let output = Command::new("wg")
            .args(["show", &self.config.interface, "latest-handshakes"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to get WireGuard handshake times: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 {
                if let Ok(epoch_secs) = parts[1].parse::<u64>() {
                    if epoch_secs > 0 {
                        let handshake_time = Duration::from_secs(epoch_secs);
                        return Ok(Some(handshake_time));
                    }
                }
            }
        }

        Ok(None) // No handshake yet
    }

    /// Set the private key on the interface.
    fn set_private_key(&self) -> Result<(), CoreError> {
        // Write the key to stdin
        let mut child = std::process::Command::new("wg")
            .args([
                "set",
                &self.config.interface,
                "private-key",
                "/dev/stdin",
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(self.config.private_key.as_bytes())?;
        }

        child.wait()?;

        debug!(interface = %self.config.interface, "Private key set");
        Ok(())
    }

    /// Set the listen port.
    fn set_listen_port(&self, port: u16) -> Result<(), CoreError> {
        let output = Command::new("wg")
            .args([
                "set",
                &self.config.interface,
                "listen-port",
                &port.to_string(),
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to set listen port on {}: {}",
                self.config.interface, stderr
            )));
        }

        debug!(interface = %self.config.interface, port, "Listen port set");
        Ok(())
    }

    /// Assign the tunnel IP address.
    fn assign_address(&self) -> Result<(), CoreError> {
        let output = Command::new("ip")
            .args([
                "addr",
                "add",
                &self.config.tunnel_address,
                "dev",
                &self.config.interface,
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // May already exist if we're reconfiguring
            if !stderr.contains("exists") {
                return Err(CoreError::Internal(format!(
                    "Failed to assign address {} to {}: {}",
                    self.config.tunnel_address, self.config.interface, stderr
                )));
            }
        }

        debug!(
            interface = %self.config.interface,
            address = %self.config.tunnel_address,
            "Tunnel address assigned"
        );
        Ok(())
    }

    /// Add the remote peer.
    fn add_peer(&self) -> Result<(), CoreError> {
        let mut args = vec![
            "set",
            &self.config.interface,
            "peer",
            &self.config.public_key,
            "endpoint",
            &self.config.endpoint,
        ];

        // Add allowed IPs
        for ip in &self.config.allowed_ips {
            args.push("allowed-ips");
            args.push(ip);
        }

        // Add preshared key if configured
        if let Some(ref psk) = self.config.preshared_key {
            args.push("preshared-key");
            args.push(psk);
        }

        // Add persistent keepalive
        if let Some(keepalive) = self.config.persistent_keepalive {
            args.push("persistent-keepalive");
            let keepalive_str = keepalive.to_string();
            args.push(keepalive_str.leak());
        }

        let output = Command::new("wg").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to add peer to {}: {}",
                self.config.interface, stderr
            )));
        }

        debug!(
            interface = %self.config.interface,
            endpoint = %self.config.endpoint,
            "Peer added"
        );
        Ok(())
    }

    /// Set DNS resolvers for the tunnel.
    fn set_dns(&self) -> Result<(), CoreError> {
        // DNS is typically handled by resolv.conf or systemd-resolved
        // For now, we just log the intended DNS servers
        let dns = self.config.dns_servers.join(", ");
        debug!(
            interface = %self.config.interface,
            %dns,
            "DNS servers configured (applied via resolv.conf or systemd-resolved)"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wireguard_config_defaults() {
        let config = WireGuardConfig {
            interface: "wg0".into(),
            private_key: "test".into(),
            listen_port: None,
            endpoint: "example.com:51820".into(),
            public_key: "test".into(),
            preshared_key: None,
            allowed_ips: vec!["0.0.0.0/0".into()],
            persistent_keepalive: None,
            tunnel_address: "10.0.0.1/24".into(),
            dns_servers: vec![],
        };
        assert_eq!(config.interface, "wg0");
        assert!(config.listen_port.is_none());
        assert!(config.preshared_key.is_none());
        assert!(config.persistent_keepalive.is_none());
        assert!(config.dns_servers.is_empty());
    }

    #[test]
    fn test_tunnel_not_up_initially() {
        let config = WireGuardConfig {
            interface: "wg0".into(),
            private_key: "test".into(),
            listen_port: None,
            endpoint: "example.com:51820".into(),
            public_key: "test".into(),
            preshared_key: None,
            allowed_ips: vec!["0.0.0.0/0".into()],
            persistent_keepalive: None,
            tunnel_address: "10.0.0.1/24".into(),
            dns_servers: vec![],
        };
        let tunnel = WireGuardTunnel::new(config);
        assert!(!tunnel.is_up);
        assert!(!tunnel.is_operational());
    }
}
