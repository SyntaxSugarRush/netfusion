// SPDX-License-Identifier: MIT OR Apache-2.0

//! Interface type detection logic.

use netfusion_shared::config::InterfaceType;
use tracing::debug;

use crate::discovery::scanner::LinkEntry;

/// Detect the type of a network interface based on its properties.
pub fn detect_interface_type(link: &LinkEntry) -> InterfaceType {
    // Check link info kind first (most reliable)
    if let Some(ref kind) = link.link_type {
        return match kind.as_str() {
            "bond" => InterfaceType::Bond,
            "bridge" => InterfaceType::Bridge,
            "vlan" => InterfaceType::Vlan,
            "tunnel" | "gre" | "ipip" | "sit" | "vti" => InterfaceType::Tunnel,
            "wireguard" => InterfaceType::WireGuard,
            "vxlan" => InterfaceType::Tunnel,
            "macvlan" | "macvtap" | "ipvlan" | "ipvtap" => InterfaceType::Virtual,
            "dummy" | "ifb" => InterfaceType::Virtual,
            "tailscale" | "tailscale0" => InterfaceType::Tailscale,
            other => {
                debug!("Unknown link type: {}", other);
                InterfaceType::Unknown
            }
        };
    }

    // Check by name patterns
    if link.name.starts_with("wg") {
        return InterfaceType::WireGuard;
    }
    if link.name == "tailscale0" || link.name.starts_with("ts-") {
        return InterfaceType::Tailscale;
    }
    if link.name.starts_with("tun") || link.name.starts_with("tap") {
        return InterfaceType::Tunnel;
    }
    if link.name.starts_with("ppp") || link.name.starts_with("pppoe") {
        return InterfaceType::Ppp;
    }
    if link.name.starts_with("usb") && (link.name.contains("tether") || link.name.contains("rndis")) {
        return InterfaceType::UsbTether;
    }
    if link.name.starts_with("wwan") || link.name.starts_with("ttyACM") || link.name.starts_with("cdc") {
        return InterfaceType::Cellular;
    }

    // Check by name whether it looks like wireless
    if link.name.starts_with("wl") || link.name.starts_with("wifi") || link.name.starts_with("ra") {
        return InterfaceType::Wireless;
    }

    // Default: if it has a MAC and isn't one of the above, it's likely ethernet
    if link.mac.is_some() && link.name != "lo" {
        return InterfaceType::Ethernet;
    }

    InterfaceType::Unknown
}

/// Check if an interface is managed by NetworkManager via sysfs/DBus.
/// Returns true if NM is actively managing this interface.
pub fn is_nm_managed(_interface_name: &str) -> bool {
    // TODO: Implement via zbus + org.freedesktop.NetworkManager
    // Query NM DBus API:
    // 1. Get all devices
    // 2. Check if interface name matches
    // 3. Check device managed state
    false
}

/// Detect which network management stack is active on the system.
pub fn detect_network_stack() -> NetworkStack {
    // Check for NetworkManager
    if std::path::Path::new("/var/run/NetworkManager/NetworkManager.pid").exists() {
        return NetworkStack::NetworkManager;
    }

    // Check for systemd-networkd
    if std::path::Path::new("/run/systemd/netif").exists() {
        return NetworkStack::SystemdNetworkd;
    }

    // Check for netplan
    if std::path::Path::new("/etc/netplan").exists() {
        return NetworkStack::Netplan;
    }

    // Check for traditional ifupdown
    if std::path::Path::new("/etc/network/interfaces").exists() {
        return NetworkStack::Ifupdown;
    }

    NetworkStack::Unknown
}

/// Detected network management stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStack {
    NetworkManager,
    SystemdNetworkd,
    Netplan,
    Ifupdown,
    Unknown,
}

impl std::fmt::Display for NetworkStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NetworkManager => write!(f, "NetworkManager"),
            Self::SystemdNetworkd => write!(f, "systemd-networkd"),
            Self::Netplan => write!(f, "netplan"),
            Self::Ifupdown => write!(f, "ifupdown"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}
