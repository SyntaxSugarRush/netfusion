// SPDX-License-Identifier: MIT OR Apache-2.0

//! Detailed metadata collection for discovered interfaces.
//!
//! Collects ethtool data (speed, duplex, driver), wireless stats,
//! and other information not available via rtnetlink.

use futures::TryStreamExt;
use netfusion_shared::types::Duplex;
use netfusion_shared::types::{InterfaceInfo, WirelessInfo};
use tracing::{debug, warn};

/// Collect detailed metadata for a discovered interface.
///
/// This function supplements the basic rtnetlink scan with:
/// - ethtool data (speed, duplex, driver)
/// - Wireless signal/noise/SSID (via iw/sysfs)
pub async fn collect_detailed_metadata(interface: &mut InterfaceInfo) {
    collect_ethtool_data(interface).await;
    collect_wireless_data(interface);
}

/// Collect speed, duplex, and driver info via ethtool netlink.
async fn collect_ethtool_data(interface: &mut InterfaceInfo) {
    // Create ethtool connection
    let (connection, mut handle, _) = match ethtool::new_connection() {
        Ok(c) => c,
        Err(e) => {
            debug!("ethtool connection failed for {}: {}", interface.name, e);
            return;
        }
    };
    tokio::spawn(connection);

    let iface_name = interface.name.clone();
    let mut link_mode_handle = handle.link_mode().get(Some(&iface_name)).execute().await;

    while let Ok(Some(msg)) = link_mode_handle.try_next().await {
        // The ethtool message is wrapped in GenlMessage, attributes are in payload.nlas
        for attr in msg.payload.nlas {
            match attr {
                ethtool::EthtoolAttr::LinkMode(link_mode_attr) => {
                    match link_mode_attr {
                        ethtool::EthtoolLinkModeAttr::Speed(speed) => {
                            if speed > 0 {
                                interface.speed_mbps = Some(speed as u64);
                            }
                        }
                        ethtool::EthtoolLinkModeAttr::Duplex(duplex) => {
                            interface.duplex = match duplex {
                                ethtool::EthtoolLinkModeDuplex::Full => Some(Duplex::Full),
                                ethtool::EthtoolLinkModeDuplex::Half => Some(Duplex::Half),
                                ethtool::EthtoolLinkModeDuplex::Unknown => Some(Duplex::Unknown),
                                _ => None,
                            };
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    if let (Some(speed), Some(duplex)) = (interface.speed_mbps, interface.duplex) {
        debug!(
            "ethtool: {} - {} Mbps, {:?} duplex",
            interface.name, speed, duplex
        );
    }

    // Get driver info from sysfs
    let driver_path = format!("/sys/class/net/{}/device/driver", interface.name);
    if let Ok(driver) = std::fs::read_link(&driver_path) {
        if let Some(driver_name) = driver.file_name() {
            interface.driver = Some(driver_name.to_string_lossy().into_owned());
            debug!("Driver for {}: {:?}", interface.name, interface.driver);
        }
    }
}

/// Collect wireless signal quality and connection data.
fn collect_wireless_data(interface: &mut InterfaceInfo) {
    // Check if this is a wireless interface
    if !is_wireless_interface(&interface.name) {
        return;
    }

    // Try cfg80211 path (modern nl80211)
    let cfg_path = format!("/sys/class/net/{}/phy80211", interface.name);
    if std::path::Path::new(&cfg_path).exists() {
        collect_cfg80211_data(interface);
        return;
    }

    // Legacy wireless stats
    let wireless_path = format!("/sys/class/net/{}/wireless", interface.name);
    if let Ok(content) = std::fs::read_to_string(&wireless_path) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0] {
                    "Signal.level" => {
                        if let Ok(level) = parts[1].parse::<i8>() {
                            let wireless = interface.wireless.get_or_insert_with(WirelessInfo::default);
                            wireless.signal_dbm = Some(level);
                        }
                    }
                    "Noise.level" => {
                        if let Ok(level) = parts[1].parse::<i8>() {
                            let wireless = interface.wireless.get_or_insert_with(WirelessInfo::default);
                            wireless.noise_dbm = Some(level);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Collect wireless data from cfg80211 (modern nl80211 API).
fn collect_cfg80211_data(interface: &mut InterfaceInfo) {
    // Get wireless info from iw command
    if let Ok(output) =
        std::process::Command::new("iw").args(["dev", &interface.name, "info"]).output()
    {
        let output = String::from_utf8_lossy(&output.stdout);
        let wireless = interface.wireless.get_or_insert_with(WirelessInfo::default);

        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(ssid_value) = trimmed.strip_prefix("ssid ") {
                let ssid_value = ssid_value.trim();
                if !ssid_value.is_empty() {
                    wireless.ssid = Some(ssid_value.to_string());
                }
            }
            if trimmed.starts_with("channel ") {
                if let Some(channel_info) = trimmed.strip_prefix("channel ") {
                    let parts: Vec<&str> = channel_info.split_whitespace().collect();
                    if let Some(freq_str) = parts.first() {
                        if let Ok(freq) = freq_str.parse::<u32>() {
                            wireless.frequency_mhz = Some(freq);
                        }
                    }
                }
            }
            if let Some(signal_str) = trimmed.strip_prefix("signal: ") {
                if let Ok(signal) = signal_str.trim_end_matches(" dBm").parse::<i8>() {
                    wireless.signal_dbm = Some(signal);
                }
            }
        }
    } else {
        warn!("iw command not available for wireless stats on {}", interface.name);
    }
}

/// Check if an interface is likely to be wireless based on its name.
fn is_wireless_interface(name: &str) -> bool {
    name.starts_with("wl")
        || name.starts_with("wifi")
        || name.starts_with("ra")
        || name.starts_with("ath")
        || name.starts_with("wlp")
        || name.starts_with("wlan")
}
