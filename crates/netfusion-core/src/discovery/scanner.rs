// SPDX-License-Identifier: MIT OR Apache-2.0

//! Core interface scanner using rtnetlink.

use std::collections::HashMap;

use futures::TryStreamExt;
use netlink_packet_route::link::{LinkAttribute, LinkFlag, LinkInfo, State};
use netlink_packet_route::address::AddressAttribute as AddrAttribute;
use netlink_packet_route::route::RouteAttribute;
use netfusion_shared::types::{InterfaceInfo, InterfaceStats, IpInfo, LinkState};
use rtnetlink::{new_connection, Handle, IpVersion};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::discovery::metadata::collect_detailed_metadata;
use crate::discovery::type_detection::detect_interface_type;

/// Errors that can occur during interface scanning.
#[derive(Debug, Error)]
pub enum ScanError {
    #[error("rtnetlink connection failed: {0}")]
    NetlinkError(#[source] std::io::Error),

    #[error("failed to enumerate links: {0}")]
    EnumerateLinksError(#[source] rtnetlink::Error),

    #[error("failed to enumerate addresses: {0}")]
    EnumerateAddressesError(#[source] rtnetlink::Error),

    #[error("failed to enumerate routes: {0}")]
    EnumerateRoutesError(#[source] rtnetlink::Error),
}

/// Scanner that discovers all network interfaces on the system.
pub struct InterfaceScanner {
    handle: Handle,
}

impl InterfaceScanner {
    /// Create a new scanner by establishing a netlink connection.
    pub fn new() -> Result<Self, ScanError> {
        let (connection, handle, _) =
            new_connection().map_err(ScanError::NetlinkError)?;
        tokio::spawn(connection);
        Ok(Self { handle })
    }

    /// Scan all interfaces and return comprehensive information.
    pub async fn scan(&self) -> Result<Vec<InterfaceInfo>, ScanError> {
        info!("Starting interface discovery scan");

        let links = self.enumerate_links().await?;
        let addresses = self.enumerate_addresses().await?;
        let routes = self.enumerate_routes().await?;
        let stats = self.collect_stats().await?;

        // Build a map of interface index -> addresses
        let addr_by_index: HashMap<u32, Vec<&AddressEntry>> = {
            let mut map: HashMap<u32, Vec<&AddressEntry>> = HashMap::new();
            for addr in &addresses {
                map.entry(addr.index).or_default().push(addr);
            }
            map
        };

        // Build a map of interface index -> default gateway
        let default_gateway_by_index: HashMap<u32, String> = routes
            .iter()
            .filter(|r| r.dst_prefix_len == 0)
            .filter_map(|r| {
                r.if_index
                    .map(|idx| (idx, r.gateway.clone().unwrap_or_default()))
            })
            .collect();

        let mut interfaces = Vec::new();

        for link in &links {
            // Skip loopback
            if link.name == "lo" {
                debug!("Skipping loopback interface");
                continue;
            }

            // Skip virtual/docker interfaces by default
            if is_virtual_interface(&link.name) {
                debug!("Skipping virtual interface: {}", link.name);
                continue;
            }

            let if_type = detect_interface_type(link);
            let link_addresses = addr_by_index.get(&link.index).cloned().unwrap_or_default();

            let ip_addresses: Vec<IpInfo> = link_addresses
                .iter()
                .map(|a| IpInfo {
                    cidr: format!("{}/{}", a.address, a.prefix_len),
                    dhcp: false, // TODO: detect via DHCP info
                })
                .collect();

            let gateway = default_gateway_by_index.get(&link.index).cloned();

            let mut interface = InterfaceInfo {
                name: link.name.clone(),
                if_type,
                mac: link.mac.clone(),
                driver: None,
                speed_mbps: None,
                duplex: None,
                mtu: link.mtu,
                addresses: ip_addresses,
                gateway,
                dns_servers: Vec::new(),
                link_state: link.state,
                managed: false,
                nm_managed: false,
                wireless: None,
                cellular: None,
                stats: stats.get(&link.index).cloned().unwrap_or_default(),
                health: None,
                last_seen: None,
            };

            // Collect detailed metadata (ethtool, wireless)
            collect_detailed_metadata(&mut interface).await;

            debug!(
                "Discovered interface: {} (type: {:?}, state: {:?})",
                interface.name, interface.if_type, interface.link_state
            );
            interfaces.push(interface);
        }

        info!("Discovered {} interfaces", interfaces.len());
        Ok(interfaces)
    }

    /// Enumerate all network links.
    async fn enumerate_links(&self) -> Result<Vec<LinkEntry>, ScanError> {
        let mut links = Vec::new();

        let mut handle = self.handle.link().get().execute();
        while let Some(msg) =
            handle.try_next().await.map_err(ScanError::EnumerateLinksError)?
        {
            let header = msg.header;
            let attrs = msg.attributes;

            let name = attrs
                .iter()
                .find_map(|a| match a {
                    LinkAttribute::IfName(n) => Some(n.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| format!("unknown-{}", header.index));

            let mac = attrs.iter().find_map(|a| match a {
                LinkAttribute::Address(addr) => Some(
                    addr.iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<Vec<_>>()
                        .join(":"),
                ),
                _ => None,
            });

            let mtu = attrs
                .iter()
                .find_map(|a| match a {
                    LinkAttribute::Mtu(m) => Some(*m),
                    _ => None,
                })
                .unwrap_or(1500);

            // Check IFF_UP flag
            let up = header.flags.contains(&LinkFlag::Up);

            let state = if up {
                LinkState::Up
            } else {
                LinkState::Down
            };

            // Refine state based on operstate
            let operstate = attrs.iter().find_map(|a| match a {
                LinkAttribute::OperState(s) => Some(s.clone()),
                _ => None,
            });

            let state = match operstate {
                Some(State::Down) => LinkState::Down,
                Some(State::LowerLayerDown) | Some(State::Dormant) => LinkState::Up,
                Some(State::Up) => LinkState::Up,
                _ => state,
            };

            links.push(LinkEntry {
                index: header.index,
                name,
                mac,
                mtu,
                state,
                link_type: attrs.iter().find_map(|a| match a {
                    LinkAttribute::LinkInfo(info) => info
                        .iter()
                        .find_map(|inner| match inner {
                            LinkInfo::Kind(k) => Some(format!("{:?}", k)),
                            _ => None,
                        }),
                    _ => None,
                }),
            });
        }

        Ok(links)
    }

    /// Enumerate all IP addresses.
    async fn enumerate_addresses(&self) -> Result<Vec<AddressEntry>, ScanError> {
        let mut addresses = Vec::new();

        let mut handle = self.handle.address().get().execute();
        while let Some(msg) = handle
            .try_next()
            .await
            .map_err(ScanError::EnumerateAddressesError)?
        {
            let index = msg.header.index;
            let prefix_len = msg.header.prefix_len;

            let address = msg.attributes.iter().find_map(|a| match a {
                AddrAttribute::Address(addr) => Some(*addr),
                _ => None,
            });

            if let Some(addr) = address {
                addresses.push(AddressEntry {
                    index,
                    address: addr,
                    prefix_len,
                });
            }
        }

        Ok(addresses)
    }

    /// Enumerate routes to find default gateways.
    async fn enumerate_routes(&self) -> Result<Vec<RouteEntry>, ScanError> {
        let mut routes = Vec::new();

        let mut handle = self.handle.route().get(IpVersion::V4).execute();
        while let Some(msg) = handle
            .try_next()
            .await
            .map_err(ScanError::EnumerateRoutesError)?
        {
            let attrs = msg.attributes;

            let gateway = attrs.iter().find_map(|a| match a {
                RouteAttribute::Gateway(gw) => {
                    use netlink_packet_route::route::RouteAddress;
                    match gw {
                        RouteAddress::Inet(addr) => Some(addr.to_string()),
                        RouteAddress::Inet6(addr) => Some(addr.to_string()),
                        _ => None,
                    }
                }
                _ => None,
            });

            let dst_prefix_len = msg.header.destination_prefix_length;

            let if_index = attrs.iter().find_map(|a| match a {
                RouteAttribute::Oif(idx) => Some(*idx),
                _ => None,
            });

            routes.push(RouteEntry {
                gateway,
                dst_prefix_len,
                if_index,
            });
        }

        Ok(routes)
    }

    /// Collect interface statistics from /proc/net/dev.
    async fn collect_stats(
        &self,
    ) -> Result<HashMap<u32, InterfaceStats>, ScanError> {
        let mut stats = HashMap::new();

        let net_dev = match std::fs::read_to_string("/proc/net/dev") {
            Ok(content) => content,
            Err(e) => {
                warn!("Failed to read /proc/net/dev: {}", e);
                return Ok(stats);
            }
        };

        for line in net_dev.lines().skip(2) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 17 {
                continue;
            }

            let name = parts[0].trim_end_matches(':');
            let index = name_to_index(name).unwrap_or(0);
            if index == 0 {
                continue;
            }

            stats.insert(
                index,
                InterfaceStats {
                    rx_bytes: parts[1].parse().unwrap_or(0),
                    rx_packets: parts[2].parse().unwrap_or(0),
                    rx_errors: parts[3].parse().unwrap_or(0),
                    rx_dropped: parts[4].parse().unwrap_or(0),
                    tx_bytes: parts[9].parse().unwrap_or(0),
                    tx_packets: parts[10].parse().unwrap_or(0),
                    tx_errors: parts[11].parse().unwrap_or(0),
                    tx_dropped: parts[12].parse().unwrap_or(0),
                },
            );
        }

        Ok(stats)
    }
}

/// Get interface index by name from sysfs.
fn name_to_index(name: &str) -> Option<u32> {
    let path = format!("/sys/class/net/{}/ifindex", name);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Check if an interface name suggests a virtual/docker/container interface.
fn is_virtual_interface(name: &str) -> bool {
    name.starts_with("docker")
        || name.starts_with("br-")
        || name.starts_with("veth")
        || name.starts_with("virbr")
        || name.starts_with("lxcbr")
        || name.starts_with("cali")
        || name.starts_with("flannel")
        || name.starts_with("cni")
        || name.starts_with("podman")
}

/// Internal representation of a discovered link.
pub struct LinkEntry {
    pub index: u32,
    pub name: String,
    pub mac: Option<String>,
    pub mtu: u32,
    pub state: LinkState,
    pub link_type: Option<String>,
}

/// Internal representation of an address entry.
struct AddressEntry {
    index: u32,
    address: std::net::IpAddr,
    prefix_len: u8,
}

/// Internal representation of a route entry.
struct RouteEntry {
    gateway: Option<String>,
    dst_prefix_len: u8,
    if_index: Option<u32>,
}
