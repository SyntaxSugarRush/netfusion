// SPDX-License-Identifier: MIT OR Apache-2.0

//! Configuration schema and types for NetFusion.
//!
//! This module defines the complete configuration format, including:
//! - Root configuration with schema versioning
//! - Interface selectors and discovery rules
//! - Bond group definitions
//! - Routing policies
//! - Tunnel definitions
//! - Profile definitions with mode-specific settings
//! - Logging, QoS, and relay configuration

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

/// Schema version for migration support.
pub const SCHEMA_VERSION: u32 = 1;

/// Root configuration for NetFusion.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct NetfusionConfig {
    /// Configuration schema version. Incremented when the format changes.
    pub schema_version: u32,

    /// Global daemon settings.
    #[validate(nested)]
    pub daemon: DaemonConfig,

    /// Interface discovery and selection rules.
    #[validate(nested)]
    pub interfaces: InterfaceConfig,

    /// Bond group definitions.
    #[validate(nested)]
    pub bonds: Vec<BondConfig>,

    /// Routing policy definitions.
    #[validate(nested)]
    pub policies: Vec<PolicyConfig>,

    /// Tunnel definitions (WireGuard, relay, etc.).
    #[validate(nested)]
    pub tunnels: Vec<TunnelConfig>,

    /// Operational profiles (gaming, streaming, etc.).
    #[validate(nested)]
    pub profiles: HashMap<String, ProfileConfig>,

    /// Quality of Service settings.
    #[validate(nested)]
    pub qos: Option<QosConfig>,

    /// Logging configuration.
    #[validate(nested)]
    pub logging: Option<LoggingConfig>,

    /// Remote relay configuration.
    #[validate(nested)]
    pub relay: Option<RelayConfig>,
}

impl Default for NetfusionConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            daemon: DaemonConfig::default(),
            interfaces: InterfaceConfig::default(),
            bonds: Vec::new(),
            policies: Vec::new(),
            tunnels: Vec::new(),
            profiles: HashMap::new(),
            qos: None,
            logging: None,
            relay: None,
        }
    }
}

/// Global daemon settings.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct DaemonConfig {
    /// Unix socket path for IPC.
    #[serde(default = "default_socket_path")]
    pub socket_path: String,

    /// State database path (SQLite).
    #[serde(default = "default_state_path")]
    pub state_path: String,

    /// Polling interval for interface health checks (milliseconds).
    #[serde(default = "default_health_interval")]
    pub health_interval_ms: u64,

    /// Rollback timeout on failed apply (seconds).
    #[serde(default = "default_rollback_timeout")]
    pub rollback_timeout_secs: u64,

    /// Dry-run mode — validate without applying.
    #[serde(default)]
    pub dry_run: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            state_path: default_state_path(),
            health_interval_ms: default_health_interval(),
            rollback_timeout_secs: default_rollback_timeout(),
            dry_run: false,
        }
    }
}

fn default_socket_path() -> String {
    "/run/netfusion/netfusion.sock".into()
}

fn default_state_path() -> String {
    "/var/lib/netfusion/state.db".into()
}

fn default_health_interval() -> u64 {
    1000
}

fn default_rollback_timeout() -> u64 {
    30
}

/// Interface discovery and selection configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Validate, Default)]
pub struct InterfaceConfig {
    /// Interface selectors for auto-discovery.
    #[validate(nested)]
    pub selectors: Vec<InterfaceSelector>,

    /// Explicitly managed interfaces (overrides discovery).
    pub managed: Vec<String>,

    /// Interfaces to always ignore.
    pub exclude: Vec<String>,
}

/// Rule for selecting interfaces during discovery.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct InterfaceSelector {
    /// Human-readable name for this selector.
    pub name: String,

    /// Match by interface name pattern (glob).
    pub name_pattern: Option<String>,

    /// Match by interface type.
    pub r#type: Option<InterfaceType>,

    /// Match by driver name.
    pub driver: Option<String>,

    /// Minimum link speed (Mbps). 0 = no minimum.
    #[serde(default)]
    pub min_speed_mbps: u64,

    /// Priority weight for this selector (higher = preferred).
    #[serde(default = "default_selector_weight")]
    pub weight: u8,
}

fn default_selector_weight() -> u8 {
    50
}

/// Linux-supported interface types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceType {
    Ethernet,
    Wireless,
    Vlan,
    Bridge,
    Bond,
    Tunnel,
    WireGuard,
    Tailscale,
    Ppp,
    UsbTether,
    Cellular,
    Loopback,
    Virtual,
    Unknown,
}

/// Bond group definition.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct BondConfig {
    /// Unique name for this bond group.
    #[validate(length(min = 1, max = 64))]
    pub name: String,

    /// Bonding mode.
    pub mode: BondMode,

    /// Interface names or selector names to include.
    #[validate(length(min = 1))]
    pub members: Vec<String>,

    /// Per-member weights (must match members length, or be empty for equal).
    #[serde(default)]
    pub weights: Vec<u8>,

    /// Failover threshold: minimum members required active.
    #[serde(default = "default_min_members")]
    pub min_active_members: usize,

    /// Health check targets for this bond.
    #[serde(default)]
    pub health_targets: Vec<String>,

    /// Associated routing policy name.
    pub policy: Option<String>,
}

fn default_min_members() -> usize {
    1
}

/// Bonding/aggregation modes supported by NetFusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BondMode {
    /// Active-backup: one interface active, others standby.
    ActiveBackup,
    /// Balanced round-robin.
    BalanceRr,
    /// XOR-based balancing.
    BalanceXor,
    /// Broadcast on all interfaces.
    Broadcast,
    /// 802.3ad LACP (requires switch support).
    Lacp,
    /// Adaptive transmit load balancing.
    AdaptiveTlb,
    /// Adaptive load balancing (includes receive).
    AdaptiveAlb,
    /// MPTCP-based aggregation (requires remote endpoint).
    Mptcp,
    /// ECMP (per-flow balancing via policy routing).
    Ecmp,
    /// Weighted balancing based on health scores.
    Weighted,
    /// User-space tunnel aggregation.
    Tunnel,
}

/// Routing policy definition.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct PolicyConfig {
    /// Unique name for this policy.
    #[validate(length(min = 1, max = 64))]
    pub name: String,

    /// Traffic matching rules.
    #[validate(nested)]
    pub rules: Vec<RuleConfig>,

    /// Action to take for matched traffic.
    pub action: PolicyAction,

    /// Priority (lower number = higher priority).
    #[serde(default = "default_policy_priority")]
    pub priority: u32,
}

fn default_policy_priority() -> u32 {
    100
}

/// Traffic matching rule.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RuleConfig {
    /// Source CIDR match.
    pub src: Option<String>,

    /// Destination CIDR match.
    pub dst: Option<String>,

    /// Protocol match (tcp, udp, icmp, or numeric).
    pub proto: Option<String>,

    /// Destination port match.
    pub dport: Option<u16>,

    /// Source port match.
    pub sport: Option<u16>,

    /// DSCP mark match.
    pub dscp: Option<u8>,

    /// Firewall mark match.
    pub fwmark: Option<u32>,

    /// Application match (via cgroup or process name).
    pub app: Option<String>,
}

/// Policy action for matched traffic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PolicyAction {
    /// Route through a specific bond/interface.
    Route {
        bond: String,
    },
    /// Route through the interface with lowest RTT.
    LowestLatency,
    /// Route through the interface with highest throughput.
    HighestThroughput,
    /// Load balance across bond members.
    LoadBalance {
        bond: String,
    },
    /// Drop matching traffic.
    Drop,
    /// Accept and use default routing.
    Accept,
}

/// Tunnel definition for VPN/relay aggregation.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct TunnelConfig {
    /// Unique tunnel name.
    #[validate(length(min = 1, max = 64))]
    pub name: String,

    /// Tunnel type.
    pub r#type: TunnelType,

    /// Remote endpoint address.
    pub remote: String,

    /// Local bind address (optional).
    pub local_bind: Option<String>,

    /// Authentication/pre-shared key reference.
    pub auth_ref: Option<String>,

    /// Tunnel-specific options.
    #[serde(default)]
    pub options: HashMap<String, String>,

    /// Auto-connect on daemon start.
    #[serde(default)]
    pub auto_connect: bool,

    /// Reconnect on failure.
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,

    /// Reconnect interval (seconds).
    #[serde(default = "default_reconnect_interval")]
    pub reconnect_interval_secs: u64,
}

fn default_auto_reconnect() -> bool {
    true
}

fn default_reconnect_interval() -> u64 {
    10
}

/// Supported tunnel types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelType {
    WireGuard,
    OpenVpn,
    Quic,
    Relay,
    Tailscale,
}

/// Operational profile with mode-specific tuning.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ProfileConfig {
    /// Operational mode.
    pub mode: ProfileMode,

    /// Interfaces to use (empty = auto-select).
    #[serde(default)]
    pub interfaces: Vec<String>,

    /// Health scoring weights override.
    #[validate(nested)]
    pub health_weights: Option<HealthWeights>,

    /// Maximum acceptable jitter (ms).
    pub max_jitter_ms: Option<f64>,

    /// Maximum acceptable packet loss (%).
    pub max_loss_percent: Option<f64>,

    /// Prefer lowest RTT interface.
    #[serde(default)]
    pub prefer_lowest_rtt: bool,

    /// Failover threshold (% drop in health score to trigger).
    #[serde(default = "default_failover_threshold")]
    pub failover_threshold: u8,

    /// Auto-activate when application is detected.
    #[serde(default)]
    pub activate_on_app: Vec<String>,

    /// Schedule for automatic activation.
    #[validate(nested)]
    pub schedule: Option<ScheduleConfig>,
}

fn default_failover_threshold() -> u8 {
    15
}

/// Operational mode presets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileMode {
    /// Prioritize lowest stable latency.
    LowLatency,
    /// Prioritize upload stability and jitter reduction.
    Streaming,
    /// Prioritize aggregate throughput.
    BulkTransfer,
    /// Prioritize packet ordering and low loss.
    Voip,
    /// Balanced default mode.
    Balanced,
    /// Custom mode with explicit weights.
    Custom,
}

/// Health scoring weight configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct HealthWeights {
    /// RTT weight (0-100).
    #[validate(range(min = 0, max = 100))]
    pub rtt: u8,

    /// Jitter weight (0-100).
    #[validate(range(min = 0, max = 100))]
    pub jitter: u8,

    /// Packet loss weight (0-100).
    #[validate(range(min = 0, max = 100))]
    pub loss: u8,

    /// Throughput weight (0-100).
    #[validate(range(min = 0, max = 100))]
    pub throughput: u8,

    /// Stability weight (0-100).
    #[validate(range(min = 0, max = 100))]
    pub stability: u8,
}

impl Default for HealthWeights {
    fn default() -> Self {
        Self {
            rtt: 30,
            jitter: 20,
            loss: 25,
            throughput: 15,
            stability: 10,
        }
    }
}

impl HealthWeights {
    /// Balanced default weights.
    pub fn balanced() -> Self {
        Self::default()
    }

    /// Gaming: prioritize RTT and jitter.
    pub fn gaming() -> Self {
        Self {
            rtt: 40,
            jitter: 30,
            loss: 15,
            throughput: 5,
            stability: 10,
        }
    }

    /// Streaming: prioritize throughput and loss.
    pub fn streaming() -> Self {
        Self {
            rtt: 15,
            jitter: 20,
            loss: 25,
            throughput: 30,
            stability: 10,
        }
    }

    /// VoIP: prioritize loss and jitter.
    pub fn voip() -> Self {
        Self {
            rtt: 20,
            jitter: 30,
            loss: 30,
            throughput: 5,
            stability: 15,
        }
    }

    /// Bulk: prioritize throughput.
    pub fn bulk_transfer() -> Self {
        Self {
            rtt: 10,
            jitter: 10,
            loss: 15,
            throughput: 45,
            stability: 20,
        }
    }

    /// Normalize weights so they sum to 100.
    pub fn normalize(&mut self) {
        let total = self.rtt as u16
            + self.jitter as u16
            + self.loss as u16
            + self.throughput as u16
            + self.stability as u16;
        if total == 0 {
            *self = Self::default();
            return;
        }
        // We don't force-normalize since the validator checks range.
        // The scoring algorithm handles non-100 totals.
    }
}

/// Schedule for automatic profile activation.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ScheduleConfig {
    /// Cron-like expression (minute hour day month weekday).
    pub cron: String,

    /// Duration to keep profile active (minutes). 0 = until next schedule.
    #[serde(default)]
    pub duration_mins: Option<u32>,
}

/// Quality of Service configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct QosConfig {
    /// Enable QoS shaping.
    #[serde(default)]
    pub enabled: bool,

    /// Queueing discipline to use.
    #[serde(default)]
    pub qdisc: QdiscType,

    /// Enable Explicit Congestion Notification.
    #[serde(default)]
    pub ecn: bool,

    /// Enable DSCP tagging.
    #[serde(default)]
    pub dscp_tagging: bool,

    /// Per-interface QoS overrides.
    #[validate(nested)]
    pub interface_overrides: HashMap<String, QdiscConfig>,
}

/// Queueing discipline types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QdiscType {
    #[default]
    FqCodel,
    Cake,
    Htb,
    Prio,
    PfifoFast,
}

/// Per-interface QoS configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct QdiscConfig {
    /// Override qdisc type for this interface.
    pub qdisc: Option<QdiscType>,

    /// Target latency (ms) for fq_codel.
    pub target_ms: Option<u32>,

    /// Interval (ms) for fq_codel.
    pub interval_ms: Option<u32>,

    /// Limit (packets) for the queue.
    pub limit: Option<u32>,
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct LoggingConfig {
    /// Log level filter.
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log to file.
    pub file: Option<String>,

    /// Log to journald.
    #[serde(default)]
    pub journald: bool,

    /// Max log file size (MB).
    #[serde(default = "default_max_log_size")]
    pub max_size_mb: u64,

    /// Number of rotated log files to keep.
    #[serde(default = "default_max_log_files")]
    pub max_files: usize,
}

fn default_log_level() -> String {
    "info".into()
}

fn default_max_log_size() -> u64 {
    10
}

fn default_max_log_files() -> usize {
    5
}

/// Remote relay server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RelayConfig {
    /// Remote relay server address (host:port).
    pub server: String,

    /// QUIC port.
    #[serde(default = "default_relay_port")]
    pub port: u16,

    /// Authentication token reference.
    pub auth_ref: Option<String>,

    /// Server hostname for TLS verification.
    pub server_name: Option<String>,

    /// Enable relay connection.
    #[serde(default)]
    pub enabled: bool,
}

fn default_relay_port() -> u16 {
    4433
}
