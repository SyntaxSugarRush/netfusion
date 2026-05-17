// SPDX-License-Identifier: MIT OR Apache-2.0

//! Relay server configuration.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Default bind address for the relay server.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:4433";

/// Default max concurrent connections.
const DEFAULT_MAX_CONNECTIONS: usize = 100;

/// Relay server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// QUIC bind address.
    pub bind_addr: String,

    /// Path to TLS certificate (PEM).
    pub cert_path: String,

    /// Path to TLS private key (PEM).
    pub key_path: String,

    /// Maximum concurrent client connections.
    pub max_connections: usize,

    /// Upstream target for traffic forwarding (optional).
    pub upstream: Option<String>,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            bind_addr: DEFAULT_BIND_ADDR.into(),
            cert_path: "/etc/netfusion/relay/cert.pem".into(),
            key_path: "/etc/netfusion/relay/key.pem".into(),
            max_connections: DEFAULT_MAX_CONNECTIONS,
            upstream: None,
        }
    }
}

impl RelayConfig {
    /// Load configuration from file.
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = "/etc/netfusion/relay.toml";

        if !Path::new(config_path).exists() {
            return Err(format!("Config file not found: {}", config_path).into());
        }

        let content = std::fs::read_to_string(config_path)?;
        let config: RelayConfig = toml::from_str(&content)?;
        Ok(config)
    }
}
