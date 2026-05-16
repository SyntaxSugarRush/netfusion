// SPDX-License-Identifier: MIT OR Apache-2.0

//! IPC protocol types for daemon ↔ TUI communication.
//!
//! The TUI sends requests to the daemon over a Unix domain socket.
//! The daemon responds with typed responses and can push streaming
//! updates via subscription channels.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::NetfusionConfig;
use crate::events::NetfusionEvent;
use crate::types::{BondState, HealthScore, InterfaceInfo, SystemStatus, TunnelState};

/// Request from TUI to daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRequest {
    /// Get current system status.
    GetStatus,

    /// Get list of all discovered interfaces.
    GetInterfaces,

    /// Get detailed info for a specific interface.
    GetInterface { name: String },

    /// Get all bond group states.
    GetBonds,

    /// Get state of a specific bond.
    GetBond { name: String },

    /// Get all tunnel states.
    GetTunnels,

    /// Get recent events.
    GetEvents { limit: usize },

    /// Subscribe to live event stream.
    SubscribeEvents,

    /// Unsubscribe from event stream.
    UnsubscribeEvents,

    /// Get the current active configuration.
    GetConfig,

    /// Apply a new configuration.
    ApplyConfig { config: NetfusionConfig },

    /// Dry-run apply (validate without applying).
    DryRunConfig { config: NetfusionConfig },

    /// Activate a profile.
    ActivateProfile { name: String },

    /// Deactivate current profile.
    DeactivateProfile,

    /// Get the current active profile.
    GetActiveProfile,

    /// Get health scores for an interface.
    GetHealth { interface: String },

    /// Get health scores for all interfaces.
    GetAllHealth,

    /// Trigger interface rescan.
    RescanInterfaces,

    /// Create a bond group.
    CreateBond { config: crate::config::BondConfig },

    /// Delete a bond group.
    DeleteBond { name: String },

    /// Emergency rollback.
    EmergencyRollback,

    /// Shut down the daemon.
    Shutdown,
}

/// Response from daemon to TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonResponse {
    /// Successful response with data.
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<ResponseData>,
    },

    /// Error response.
    Error {
        message: String,
        recoverable: bool,
    },
}

/// Typed response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseData {
    Status(SystemStatus),
    Interfaces(Vec<InterfaceInfo>),
    Interface(InterfaceInfo),
    Bonds(Vec<BondState>),
    Bond(BondState),
    Tunnels(Vec<TunnelState>),
    Events(Vec<NetfusionEvent>),
    EventStream(NetfusionEvent),
    Config(NetfusionConfig),
    Profile(Option<String>),
    Health(HealthScore),
    AllHealth(Vec<(String, HealthScore)>),
    BondCreated(String),
    BondDeleted(String),
    Empty,
}

/// Streaming event pushed from daemon to TUI (independent of request/response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonPush {
    /// Unique message ID for deduplication.
    pub id: Uuid,

    /// Timestamp.
    pub timestamp: DateTime<Utc>,

    /// The pushed event.
    pub event: NetfusionEvent,
}

/// Wire protocol wrapper with versioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessage<T> {
    /// Protocol version.
    pub version: u32,

    /// Message ID for request/response correlation.
    pub id: Uuid,

    /// Timestamp.
    pub timestamp: DateTime<Utc>,

    /// Payload.
    pub payload: T,
}

/// Current IPC protocol version.
pub const IPC_PROTOCOL_VERSION: u32 = 1;

impl<T: Serialize> WireMessage<T> {
    /// Create a new wire message.
    pub fn new(payload: T) -> Self {
        Self {
            version: IPC_PROTOCOL_VERSION,
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            payload,
        }
    }
}

/// Convenience type aliases for the wire protocol.
pub type WireRequest = WireMessage<DaemonRequest>;
pub type WireResponse = WireMessage<DaemonResponse>;
pub type WirePush = WireMessage<NetfusionEvent>;

/// Encode a message using bincode for efficient transport.
pub fn encode<T: Serialize>(msg: &WireMessage<T>) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(msg)
}

/// Decode a message from bincode transport.
pub fn decode_request(bytes: &[u8]) -> Result<WireRequest, bincode::Error> {
    bincode::deserialize(bytes)
}

/// Decode a response from bincode transport.
pub fn decode_response(bytes: &[u8]) -> Result<WireResponse, bincode::Error> {
    bincode::deserialize(bytes)
}

/// Decode a push event from bincode transport.
pub fn decode_push(bytes: &[u8]) -> Result<WirePush, bincode::Error> {
    bincode::deserialize(bytes)
}
