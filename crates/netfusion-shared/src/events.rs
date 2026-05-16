// SPDX-License-Identifier: MIT OR Apache-2.0

//! Event system types for NetFusion.
//!
//! All major components subscribe to these events for reactive behavior.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// All events in the NetFusion system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetfusionEvent {
    /// An interface has come up.
    InterfaceUp(InterfaceEvent),

    /// An interface has gone down.
    InterfaceDown(InterfaceEvent),

    /// Interface health score changed significantly.
    HealthChanged(HealthEvent),

    /// Packet loss spike detected.
    PacketLossSpike(LossEvent),

    /// Congestion detected on a path.
    CongestionDetected(CongestionEvent),

    /// A failover has been triggered.
    FailoverTriggered(FailoverEvent),

    /// Failover has completed and recovered.
    FailoverRecovered(FailoverEvent),

    /// A tunnel has connected.
    TunnelConnected(TunnelEvent),

    /// A tunnel has disconnected.
    TunnelDisconnected(TunnelEvent),

    /// Routing tables have changed.
    RouteChanged(RouteEvent),

    /// A bond group membership changed.
    BondMembershipChanged(BondEvent),

    /// Configuration has been reloaded.
    ConfigReloaded(ConfigEvent),

    /// An error occurred in a subsystem.
    SubsystemError(ErrorEvent),

    /// A profile has been activated.
    ProfileActivated(ProfileEvent),

    /// A profile has been deactivated.
    ProfileDeactivated(ProfileEvent),
}

impl NetfusionEvent {
    /// Get the timestamp of this event.
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::InterfaceUp(e) => e.timestamp,
            Self::InterfaceDown(e) => e.timestamp,
            Self::HealthChanged(e) => e.timestamp,
            Self::PacketLossSpike(e) => e.timestamp,
            Self::CongestionDetected(e) => e.timestamp,
            Self::FailoverTriggered(e) => e.timestamp,
            Self::FailoverRecovered(e) => e.timestamp,
            Self::TunnelConnected(e) => e.timestamp,
            Self::TunnelDisconnected(e) => e.timestamp,
            Self::RouteChanged(e) => e.timestamp,
            Self::BondMembershipChanged(e) => e.timestamp,
            Self::ConfigReloaded(e) => e.timestamp,
            Self::SubsystemError(e) => e.timestamp,
            Self::ProfileActivated(e) => e.timestamp,
            Self::ProfileDeactivated(e) => e.timestamp,
        }
    }

    /// Get a human-readable description of this event.
    pub fn description(&self) -> String {
        match self {
            Self::InterfaceUp(e) => format!("Interface {} is up", e.interface),
            Self::InterfaceDown(e) => format!("Interface {} is down", e.interface),
            Self::HealthChanged(e) => {
                format!("Interface {} health: {:.1}", e.interface, e.new_score.overall)
            }
            Self::PacketLossSpike(e) => {
                format!("Packet loss spike on {}: {:.1}%", e.interface, e.loss_percent)
            }
            Self::CongestionDetected(e) => {
                format!("Congestion on {} (queue depth: {})", e.interface, e.queue_depth)
            }
            Self::FailoverTriggered(e) => {
                format!("Failover on {} -> {}", e.bond, e.new_active.join(", "))
            }
            Self::FailoverRecovered(e) => {
                format!("Failover recovered on {}: {}", e.bond, e.new_active.join(", "))
            }
            Self::TunnelConnected(e) => format!("Tunnel {} connected", e.tunnel),
            Self::TunnelDisconnected(e) => format!("Tunnel {} disconnected", e.tunnel),
            Self::RouteChanged(e) => format!("Routes changed ({} rules)", e.rule_count),
            Self::BondMembershipChanged(e) => {
                format!("Bond {} membership changed", e.bond)
            }
            Self::ConfigReloaded(e) => format!("Config reloaded from {}", e.source),
            Self::SubsystemError(e) => {
                format!("Error in {}: {}", e.subsystem, e.message)
            }
            Self::ProfileActivated(e) => format!("Profile '{}' activated", e.profile),
            Self::ProfileDeactivated(e) => format!("Profile '{}' deactivated", e.profile),
        }
    }
}

/// Interface state change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceEvent {
    pub interface: String,
    pub timestamp: DateTime<Utc>,
    pub details: Option<String>,
}

/// Health score change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEvent {
    pub interface: String,
    pub timestamp: DateTime<Utc>,
    pub previous_score: f64,
    pub new_score: crate::types::HealthScore,
}

/// Packet loss spike event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LossEvent {
    pub interface: String,
    pub timestamp: DateTime<Utc>,
    pub loss_percent: f64,
    pub duration_secs: u64,
}

/// Congestion detection event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CongestionEvent {
    pub interface: String,
    pub timestamp: DateTime<Utc>,
    pub queue_depth: u32,
    pub latency_increase_ms: f64,
}

/// Failover event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverEvent {
    pub bond: String,
    pub timestamp: DateTime<Utc>,
    pub previous_active: Vec<String>,
    pub new_active: Vec<String>,
    pub reason: String,
}

/// Tunnel state change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelEvent {
    pub tunnel: String,
    pub timestamp: DateTime<Utc>,
    pub remote: String,
    pub error: Option<String>,
}

/// Route change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEvent {
    pub timestamp: DateTime<Utc>,
    pub rule_count: usize,
    pub changes: Vec<RouteChange>,
}

/// A single route change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteChange {
    pub action: String, // "add", "del", "change"
    pub destination: Option<String>,
    pub gateway: Option<String>,
    pub interface: String,
}

/// Bond membership change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondEvent {
    pub bond: String,
    pub timestamp: DateTime<Utc>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Configuration reload event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEvent {
    pub timestamp: DateTime<Utc>,
    pub source: String,
    pub errors: Vec<String>,
}

/// Subsystem error event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub subsystem: String,
    pub timestamp: DateTime<Utc>,
    pub message: String,
    pub recoverable: bool,
}

/// Profile activation event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileEvent {
    pub profile: String,
    pub timestamp: DateTime<Utc>,
    pub trigger: String, // "manual", "schedule", "app_detection", "auto"
}
