// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tunnel management for NetFusion.
//!
//! Provides:
//! - WireGuard tunnel orchestration (create, configure, monitor)
//! - QUIC-based relay tunnels
//! - Tunnel health monitoring
//! - Auto-reconnect with backoff
//! - Integration with bonding and routing engines

pub mod wireguard;
pub mod manager;

pub use manager::TunnelManager;
pub use wireguard::{WireGuardConfig, WireGuardTunnel};
