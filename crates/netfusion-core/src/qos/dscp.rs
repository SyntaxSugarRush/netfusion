// SPDX-License-Identifier: MIT OR Apache-2.0

//! DSCP (Differentiated Services Code Point) tagging via nftables.
//!
//! Classifies traffic into DSCP classes based on protocol/port rules
//! and marks packets for QoS treatment downstream.

use tracing::{debug, info, warn};

use crate::error::CoreError;

/// DSCP traffic classes aligned with RFC 2474 / RFC 4594.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DscpClass {
    /// Low-latency data (voice, video): EF (46)
    Realtime,
    /// Interactive data (gaming, remote desktop): AF41 (34)
    Interactive,
    /// Streaming media: AF31 (26)
    Streaming,
    /// Bulk data (downloads, backups): AF21 (18)
    Bulk,
    /// Standard best-effort: CS0 (0)
    #[default]
    BestEffort,
    /// Background/scavenger: CS1 (8)
    Background,
}

impl DscpClass {
    /// Return the DSCP value (0-63).
    pub fn value(self) -> u8 {
        match self {
            DscpClass::Realtime => 46,    // EF
            DscpClass::Interactive => 34, // AF41
            DscpClass::Streaming => 26,   // AF31
            DscpClass::Bulk => 18,        // AF21
            DscpClass::BestEffort => 0,   // CS0
            DscpClass::Background => 8,   // CS1
        }
    }

    /// Return the DSCP name for logging.
    pub fn name(self) -> &'static str {
        match self {
            DscpClass::Realtime => "EF",
            DscpClass::Interactive => "AF41",
            DscpClass::Streaming => "AF31",
            DscpClass::Bulk => "AF21",
            DscpClass::BestEffort => "CS0",
            DscpClass::Background => "CS1",
        }
    }
}

/// A single traffic classification rule.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrafficRule {
    pub protocol: &'static str,
    pub dport: Option<u16>,
    pub dport_range: Option<(u16, u16)>,
    pub class: DscpClass,
    pub description: &'static str,
}

/// Default traffic classification rules.
pub fn default_classification_rules() -> Vec<TrafficRule> {
    vec![
        // Voice — real-time, highest priority
        TrafficRule {
            protocol: "udp",
            dport: Some(5060),
            class: DscpClass::Realtime,
            description: "SIP signaling",
            ..Default::default()
        },
        TrafficRule {
            protocol: "udp",
            dport_range: Some((16384, 32768)),
            class: DscpClass::Realtime,
            description: "RTP media",
            ..Default::default()
        },
        // Gaming — interactive priority
        TrafficRule {
            protocol: "udp",
            dport_range: Some((27000, 27100)),
            class: DscpClass::Interactive,
            description: "Steam gaming",
            ..Default::default()
        },
        // Streaming media
        TrafficRule {
            protocol: "tcp",
            dport_range: Some((1935, 1935)),
            class: DscpClass::Streaming,
            description: "RTMP streaming",
            ..Default::default()
        },
        // DNS — interactive
        TrafficRule {
            protocol: "udp",
            dport: Some(53),
            class: DscpClass::Interactive,
            description: "DNS resolution",
            ..Default::default()
        },
        TrafficRule {
            protocol: "tcp",
            dport: Some(53),
            class: DscpClass::Interactive,
            description: "DNS over TCP",
            ..Default::default()
        },
        // HTTPS — best effort (default)
        TrafficRule {
            protocol: "tcp",
            dport: Some(443),
            class: DscpClass::BestEffort,
            description: "HTTPS traffic",
            ..Default::default()
        },
        // HTTP — bulk
        TrafficRule {
            protocol: "tcp",
            dport: Some(80),
            class: DscpClass::Bulk,
            description: "HTTP traffic",
            ..Default::default()
        },
    ]
}

/// Generate nftables rules for DSCP tagging.
///
/// Returns a list of nft commands that can be piped to `nft -f -`.
pub fn generate_nftables_rules(_interface: &str, rules: &[TrafficRule]) -> String {
    let mut script = String::new();

    script.push_str("#!/usr/sbin/nft -f\n");
    script.push_str("# NetFusion DSCP tagging rules\n\n");

    // Create table and chain
    script.push_str("table inet netfusion {\n");
    script.push_str("  chain dscp_tagger {\n");
    script.push_str("    type filter hook output priority 0; policy accept;\n\n");

    for rule in rules {
        let dscp_val = rule.class.value();
        let mut nft_rule = format!("    {} ", rule.protocol);

        if let Some(port) = rule.dport {
            nft_rule.push_str(&format!("dport {} ", port));
        } else if let Some((start, end)) = rule.dport_range {
            nft_rule.push_str(&format!("dport {}-{} ", start, end));
        }

        nft_rule.push_str(&format!(
            "meta l4proto {} tcp dport set meta l4proto {} ip dscp set {} # {}",
            rule.protocol, rule.protocol, dscp_val, rule.description
        ));

        script.push_str(&nft_rule);
        script.push('\n');
    }

    script.push_str("  }\n");
    script.push_str("}\n");

    script
}

/// Apply DSCP tagging rules for an interface.
pub fn apply_dscp(interface: &str, rules: &[TrafficRule]) -> Result<(), CoreError> {
    let script = generate_nftables_rules(interface, rules);

    // Write to temp file and load via nft
    let tmp_path = format!("/tmp/netfusion_dscp_{}.nft", interface);
    std::fs::write(&tmp_path, &script)?;

    let output = std::process::Command::new("nft")
        .arg("-f")
        .arg(&tmp_path)
        .output()?;

    // Clean up temp file
    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::Internal(format!(
            "Failed to apply DSCP rules on {}: {}",
            interface, stderr
        )));
    }

    info!(interface, count = rules.len(), "Applied DSCP tagging rules");
    debug!(interface, rules = ?rules.iter().map(|r| r.class.name()).collect::<Vec<_>>(), "DSCP classes");
    Ok(())
}

/// Remove all NetFusion DSCP rules.
pub fn remove_dscp() -> Result<(), CoreError> {
    let output = std::process::Command::new("nft")
        .args(["delete", "table", "inet", "netfusion"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // "No such file or directory" means table doesn't exist — that's fine
        if !stderr.contains("No such file") {
            warn!(%stderr, "Failed to remove DSCP table (may be harmless)");
        }
    }

    debug!("Removed DSCP tagging table");
    Ok(())
}

/// Enable ECN on a network interface.
pub fn enable_ecn(interface: &str) -> Result<(), CoreError> {
    let output = std::process::Command::new("sysctl")
        .arg("-w")
        .arg("net.ipv4.tcp_ecn=2")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::Internal(format!(
            "Failed to enable ECN: {}",
            stderr
        )));
    }

    // Also set ECN on the interface-level qdisc if supported
    // (fq_codel already supports ECN natively — we pass it during apply)

    info!(interface, "ECN enabled (TCP level)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dscp_values() {
        assert_eq!(DscpClass::Realtime.value(), 46);
        assert_eq!(DscpClass::Interactive.value(), 34);
        assert_eq!(DscpClass::Streaming.value(), 26);
        assert_eq!(DscpClass::Bulk.value(), 18);
        assert_eq!(DscpClass::BestEffort.value(), 0);
        assert_eq!(DscpClass::Background.value(), 8);
    }

    #[test]
    fn test_default_rules_not_empty() {
        let rules = default_classification_rules();
        assert!(!rules.is_empty());
        assert!(rules.iter().any(|r| r.class == DscpClass::Realtime));
        assert!(rules.iter().any(|r| r.class == DscpClass::BestEffort));
    }

    #[test]
    fn test_nftables_script_generation() {
        let rules = default_classification_rules();
        let script = generate_nftables_rules("eth0", &rules);
        assert!(script.contains("table inet netfusion"));
        assert!(script.contains("chain dscp_tagger"));
        assert!(script.contains("dscp set 46")); // EF value
        assert!(script.contains("SIP signaling"));
    }
}
