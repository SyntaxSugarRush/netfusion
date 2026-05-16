// SPDX-License-Identifier: MIT OR Apache-2.0

//! Error type hierarchy for netfusion-core.

use thiserror::Error;

/// Top-level error for core operations.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("discovery error: {0}")]
    Discovery(#[from] DiscoveryError),

    #[error("monitoring error: {0}")]
    Monitoring(#[from] MonitoringError),

    #[error("routing error: {0}")]
    Routing(#[from] RoutingError),

    #[error("bonding error: {0}")]
    Bonding(#[from] BondingError),

    #[error("state persistence error: {0}")]
    State(#[from] StateError),

    #[error("configuration error: {0}")]
    Config(String),
}

/// Errors during interface discovery.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("failed to connect to netlink: {0}")]
    NetlinkConnect(#[source] std::io::Error),

    #[error("failed to enumerate interfaces: {0}")]
    Enumerate(#[source] std::io::Error),

    #[error("ethtool failed for interface '{iface}': {source}")]
    Ethtool {
        iface: String,
        #[source]
        source: std::io::Error,
    },

    #[error("wireless info unavailable for '{iface}'")]
    WirelessUnavailable { iface: String },
}

/// Errors during health monitoring.
#[derive(Debug, Error)]
pub enum MonitoringError {
    #[error("ping probe failed for '{iface}': {reason}")]
    PingFailed { iface: String, reason: String },

    #[error("no target available for probe on '{iface}'")]
    NoTarget { iface: String },

    #[error("probe timed out for '{iface}'")]
    Timeout { iface: String },

    #[error("interface '{iface}' is down")]
    InterfaceDown { iface: String },

    #[error("monitor channel closed for '{iface}'")]
    ChannelClosed { iface: String },
}

/// Errors during routing operations.
#[derive(Debug, Error)]
pub enum RoutingError {
    #[error("failed to modify route: {0}")]
    RouteModify(#[source] std::io::Error),

    #[error("failed to read routing table: {0}")]
    TableRead(#[source] std::io::Error),

    #[error("policy conflict: {0}")]
    PolicyConflict(String),

    #[error("rollback failed: {0}")]
    RollbackFailed(String),
}

/// Errors during bonding operations.
#[derive(Debug, Error)]
pub enum BondingError {
    #[error("failed to create bond '{name}': {source}")]
    CreateFailed {
        name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to enslave interface '{iface}' to bond '{bond}'")]
    EnslaveFailed { bond: String, iface: String },

    #[error("bond '{name}' requires at least {min} members, got {actual}")]
    InsufficientMembers { name: String, min: usize, actual: usize },

    #[error("bond '{name}' not found")]
    NotFound { name: String },
}

/// Errors during state persistence operations.
#[derive(Debug, Error)]
pub enum StateError {
    #[error("failed to open state database: {0}")]
    DbOpen(#[source] rusqlite::Error),

    #[error("failed to execute query: {0}")]
    QueryFailed(#[source] rusqlite::Error),

    #[error("state migration failed: {0}")]
    MigrationFailed(String),

    #[error("state file corrupted: {0}")]
    Corrupted(String),
}
