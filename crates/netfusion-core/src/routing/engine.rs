// SPDX-License-Identifier: MIT OR Apache-2.0

//! Routing engine — manages routes and policy routing rules.

use futures::TryStreamExt;
use netlink_packet_route::route::RouteAttribute;
use rtnetlink::{Handle, IpVersion};
use tracing::{debug, info};

use crate::error::RoutingError;

/// A route entry managed by NetFusion.
#[derive(Debug, Clone)]
pub struct ManagedRoute {
    pub destination: Option<String>,
    pub gateway: String,
    pub interface: String,
    pub metric: u32,
    pub table: Option<u32>,
}

/// The routing engine manages routes and policy routing rules.
pub struct RoutingEngine {
    handle: Handle,
    /// Routes that were added by NetFusion (for rollback).
    managed_routes: Vec<ManagedRoute>,
}

impl RoutingEngine {
    /// Create a new routing engine.
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            managed_routes: Vec::new(),
        }
    }

    /// Enumerate all current routes.
    pub async fn list_routes(&self) -> Result<Vec<ManagedRoute>, RoutingError> {
        let mut routes = Vec::new();

        let mut handle = self.handle.route().get(IpVersion::V4).execute();
        while let Some(msg) = handle
            .try_next()
            .await
            .map_err(|e| RoutingError::TableRead(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?
        {
            let attrs = &msg.attributes;

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

            let iface = attrs.iter().find_map(|a| match a {
                RouteAttribute::Oif(idx) => Some(*idx),
                _ => None,
            });

            let metric = attrs.iter().find_map(|a| match a {
                RouteAttribute::Priority(p) => Some(*p),
                _ => None,
            }).unwrap_or(0);

            if let Some(gw) = gateway {
                routes.push(ManagedRoute {
                    destination: None,
                    gateway: gw,
                    interface: iface.map(|i| i.to_string()).unwrap_or_default(),
                    metric,
                    table: None,
                });
            }
        }

        Ok(routes)
    }

    /// Add a default route via the specified gateway and interface.
    pub async fn add_default_route(
        &mut self,
        gateway: &str,
        interface: &str,
        metric: u32,
    ) -> Result<(), RoutingError> {
        info!("Adding default route via {} on {} (metric {})", gateway, interface, metric);

        let gw_addr: std::net::Ipv4Addr = gateway
            .parse()
            .map_err(|e| RoutingError::RouteModify(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;

        let if_index = self.get_interface_index(interface).await?;

        self.handle
            .route()
            .add()
            .v4()
            .destination_prefix(std::net::Ipv4Addr::UNSPECIFIED.into(), 0)
            .gateway(gw_addr)
            .output_interface(if_index)
            .priority(metric)
            .execute()
            .await
            .map_err(|e| {
                RoutingError::RouteModify(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

        self.managed_routes.push(ManagedRoute {
            destination: Some("0.0.0.0/0".into()),
            gateway: gateway.to_string(),
            interface: interface.to_string(),
            metric,
            table: None,
        });

        Ok(())
    }

    /// Delete a default route.
    pub async fn del_default_route(
        &mut self,
        gateway: &str,
        interface: &str,
        metric: u32,
    ) -> Result<(), RoutingError> {
        info!("Deleting default route via {} on {}", gateway, interface);

        let output = tokio::process::Command::new("ip")
            .args([
                "route",
                "del",
                "default",
                "via",
                gateway,
                "dev",
                interface,
                "metric",
                &metric.to_string(),
            ])
            .output()
            .await
            .map_err(|e| {
                RoutingError::RouteModify(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Non-critical: route may not exist
            debug!("Route deletion note: {}", stderr);
        }

        self.managed_routes.retain(|r| {
            !(r.gateway == gateway && r.interface == interface && r.metric == metric)
        });

        Ok(())
    }

    /// Add a policy routing rule (ip rule).
    pub async fn add_rule(
        &self,
        priority: u32,
        fwmark: Option<u32>,
        table: u32,
    ) -> Result<(), RoutingError> {
        debug!("Adding rule: priority={}, mark={:?}, table={}", priority, fwmark, table);

        let mut args = vec![
            "rule".to_string(),
            "add".to_string(),
            "prio".to_string(),
            priority.to_string(),
            "table".to_string(),
            table.to_string(),
        ];

        if let Some(mark) = fwmark {
            args.insert(2, "fwmark".to_string());
            args.insert(3, mark.to_string());
        }

        let output = tokio::process::Command::new("ip")
            .args(&args)
            .output()
            .await
            .map_err(|e| {
                RoutingError::RouteModify(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RoutingError::RouteModify(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("ip rule add failed: {}", stderr),
            )));
        }

        Ok(())
    }

    /// Delete a policy routing rule.
    pub async fn del_rule(&self, priority: u32, table: u32) -> Result<(), RoutingError> {
        debug!("Deleting rule: priority={}, table={}", priority, table);

        let output = tokio::process::Command::new("ip")
            .args([
                "rule",
                "del",
                "prio",
                &priority.to_string(),
                "table",
                &table.to_string(),
            ])
            .output()
            .await
            .map_err(|e| {
                RoutingError::RouteModify(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("Rule deletion note: {}", stderr);
        }

        Ok(())
    }

    /// Get the interface index by name.
    async fn get_interface_index(&self, name: &str) -> Result<u32, RoutingError> {
        let mut links = self.handle.link().get().match_name(name.to_string()).execute();

        match links.try_next().await {
            Ok(Some(msg)) => Ok(msg.header.index),
            Ok(None) => Err(RoutingError::RouteModify(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("interface '{}' not found", name),
            ))),
            Err(e) => Err(RoutingError::RouteModify(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))),
        }
    }

    /// Get the list of managed routes (for rollback).
    pub fn managed_routes(&self) -> &[ManagedRoute] {
        &self.managed_routes
    }

    /// Clear the managed routes list.
    pub fn clear_managed_routes(&mut self) {
        self.managed_routes.clear();
    }
}
