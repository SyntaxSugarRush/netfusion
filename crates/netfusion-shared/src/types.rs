// SPDX-License-Identifier: MIT OR Apache-2.0

//! Core data structures for NetFusion.
//!
//! Runtime representations of interfaces, bonds, health scores,
//! and route states.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{BondMode, InterfaceType};

/// Runtime information about a discovered network interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    /// Kernel interface name (e.g. "eth0", "wlan0").
    pub name: String,

    /// Interface type.
    pub if_type: InterfaceType,

    /// MAC address.
    pub mac: Option<String>,

    /// Driver name.
    pub driver: Option<String>,

    /// Negotiated link speed in Mbps.
    pub speed_mbps: Option<u64>,

    /// Duplex mode.
    pub duplex: Option<Duplex>,

    /// Maximum Transmission Unit.
    pub mtu: u32,

    /// Assigned IP addresses.
    pub addresses: Vec<IpInfo>,

    /// Default gateway (if this interface is the default route).
    pub gateway: Option<String>,

    /// DNS servers associated with this interface.
    pub dns_servers: Vec<String>,

    /// Current link state.
    pub link_state: LinkState,

    /// Whether the interface is currently managed by NetFusion.
    pub managed: bool,

    /// Whether the interface is managed by NetworkManager.
    pub nm_managed: bool,

    /// Wireless-specific metrics.
    pub wireless: Option<WirelessInfo>,

    /// Cellular-specific metrics.
    pub cellular: Option<CellularInfo>,

    /// Interface statistics from kernel.
    pub stats: InterfaceStats,

    /// Current health score.
    pub health: Option<HealthScore>,

    /// Time this interface was last seen active.
    pub last_seen: Option<DateTime<Utc>>,
}

/// IP address information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpInfo {
    /// IP address with CIDR prefix.
    pub cidr: String,

    /// Whether this address was assigned via DHCP.
    pub dhcp: bool,
}

/// Duplex mode of an interface.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Duplex {
    Full,
    Half,
    Unknown,
}

/// Current link state of an interface.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Up,
    Down,
    Unknown,
}

/// Wireless-specific interface metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirelessInfo {
    /// Signal strength in dBm.
    pub signal_dbm: Option<i8>,

    /// Noise floor in dBm.
    pub noise_dbm: Option<i8>,

    /// Signal quality percentage (0-100).
    pub quality_percent: Option<u8>,

    /// SSID of connected network.
    pub ssid: Option<String>,

    /// Frequency in MHz.
    pub frequency_mhz: Option<u32>,

    /// Channel width (MHz).
    pub channel_width_mhz: Option<u32>,

    /// Current transmission rate (Mbps).
    pub tx_rate_mbps: Option<f64>,

    /// Current reception rate (Mbps).
    pub rx_rate_mbps: Option<f64>,
}

/// Cellular modem metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellularInfo {
    /// Signal strength in dBm.
    pub signal_dbm: Option<i16>,

    /// Cell ID.
    pub cell_id: Option<String>,

    /// Network type (LTE, 5G-NR, etc.).
    pub network_type: Option<String>,

    /// RSRP (Reference Signal Received Power).
    pub rsrp: Option<i16>,

    /// RSRQ (Reference Signal Received Quality).
    pub rsrq: Option<i16>,

    /// SINR (Signal to Interference plus Noise Ratio).
    pub sinr: Option<i16>,
}

/// Kernel interface statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InterfaceStats {
    /// Bytes received.
    pub rx_bytes: u64,

    /// Bytes transmitted.
    pub tx_bytes: u64,

    /// Packets received.
    pub rx_packets: u64,

    /// Packets transmitted.
    pub tx_packets: u64,

    /// Receive errors.
    pub rx_errors: u64,

    /// Transmit errors.
    pub tx_errors: u64,

    /// Dropped packets on receive.
    pub rx_dropped: u64,

    /// Dropped packets on transmit.
    pub tx_dropped: u64,
}

/// Health score for an interface or path.
///
/// A weighted composite metric including RTT, jitter, packet loss,
/// throughput, and stability. Each component is normalized 0-100.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthScore {
    /// Overall composite score (0-100, higher is better).
    pub overall: f64,

    /// RTT component score (0-100).
    pub rtt: f64,

    /// Jitter component score (0-100).
    pub jitter: f64,

    /// Packet loss component score (0-100).
    pub loss: f64,

    /// Throughput component score (0-100).
    pub throughput: f64,

    /// Stability component score (0-100).
    pub stability: f64,

    /// Timestamp of this measurement.
    pub timestamp: DateTime<Utc>,

    /// Whether this score triggered a failover consideration.
    pub failover_candidate: bool,
}

impl HealthScore {
    /// Create a new health score from components and weights.
    /// Weights do not need to sum to 100 — they are normalized internally.
    pub fn compute(
        rtt: f64,
        jitter: f64,
        loss: f64,
        throughput: f64,
        stability: f64,
        w_rtt: u8,
        w_jitter: u8,
        w_loss: u8,
        w_throughput: u8,
        w_stability: u8,
    ) -> Self {
        let total_weight = (w_rtt + w_jitter + w_loss + w_throughput + w_stability) as f64;
        if total_weight == 0.0 {
            return Self {
                overall: 0.0,
                rtt,
                jitter,
                loss,
                throughput,
                stability,
                timestamp: Utc::now(),
                failover_candidate: false,
            };
        }

        let overall = (rtt * w_rtt as f64
            + jitter * w_jitter as f64
            + loss * w_loss as f64
            + throughput * w_throughput as f64
            + stability * w_stability as f64)
            / total_weight;

        let overall = overall.clamp(0.0, 100.0);

        Self {
            overall,
            rtt: rtt.clamp(0.0, 100.0),
            jitter: jitter.clamp(0.0, 100.0),
            loss: loss.clamp(0.0, 100.0),
            throughput: throughput.clamp(0.0, 100.0),
            stability: stability.clamp(0.0, 100.0),
            timestamp: Utc::now(),
            failover_candidate: false,
        }
    }

    /// Apply exponential moving average smoothing.
    pub fn ema(&self, previous: &Self, alpha: f64) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        Self {
            overall: self.overall * alpha + previous.overall * (1.0 - alpha),
            rtt: self.rtt * alpha + previous.rtt * (1.0 - alpha),
            jitter: self.jitter * alpha + previous.jitter * (1.0 - alpha),
            loss: self.loss * alpha + previous.loss * (1.0 - alpha),
            throughput: self.throughput * alpha + previous.throughput * (1.0 - alpha),
            stability: self.stability * alpha + previous.stability * (1.0 - alpha),
            timestamp: Utc::now(),
            failover_candidate: self.failover_candidate,
        }
    }

    /// Check if this score differs from another by more than the hysteresis threshold.
    pub fn exceeds_hysteresis(&self, other: &Self, threshold_percent: u8) -> bool {
        let diff = (self.overall - other.overall).abs();
        let threshold = threshold_percent as f64;
        diff > threshold
    }
}

/// Runtime state of a bond group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondState {
    /// Bond configuration name.
    pub name: String,

    /// Active bonding mode.
    pub mode: BondMode,

    /// Currently active member interfaces.
    pub active_members: Vec<String>,

    /// Standby member interfaces.
    pub standby_members: Vec<String>,

    /// Failed member interfaces.
    pub failed_members: Vec<String>,

    /// Current bond health (aggregate).
    pub health: Option<HealthScore>,

    /// Whether the bond is in failover state.
    pub failover_active: bool,

    /// Last failover event timestamp.
    pub last_failover: Option<DateTime<Utc>>,

    /// Bond interface name (e.g. "netfusion0").
    pub bond_interface: Option<String>,
}

/// Runtime state of a tunnel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelState {
    /// Tunnel name.
    pub name: String,

    /// Whether the tunnel is currently connected.
    pub connected: bool,

    /// Remote endpoint.
    pub remote: String,

    /// Tunnel interface name.
    pub interface: Option<String>,

    /// Last connection timestamp.
    pub connected_since: Option<DateTime<Utc>>,

    /// Bytes sent through tunnel.
    pub tx_bytes: u64,

    /// Bytes received through tunnel.
    pub rx_bytes: u64,

    /// Number of reconnection attempts.
    pub reconnect_count: u32,

    /// Last error message (if any).
    pub last_error: Option<String>,
}

/// System-wide status summary (for TUI dashboard).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    /// Total interfaces discovered.
    pub total_interfaces: usize,

    /// Number of active bond groups.
    pub active_bonds: usize,

    /// Number of connected tunnels.
    pub connected_tunnels: usize,

    /// Current active profile name.
    pub active_profile: Option<String>,

    /// System-wide health score.
    pub health: Option<HealthScore>,

    /// Whether failover is currently active.
    pub failover_active: bool,

    /// Whether the daemon is in dry-run mode.
    pub dry_run: bool,

    /// Uptime of the daemon.
    pub uptime_secs: u64,

    /// Timestamp of this status snapshot.
    pub timestamp: DateTime<Utc>,
}
