// SPDX-License-Identifier: MIT OR Apache-2.0

//! ECMP (Equal-Cost Multi-Path) routing.
//!
//! Creates multiple default routes with equal metrics to enable
//! per-flow load balancing across interfaces.

use tracing::{info, debug};

use crate::error::RoutingError;

/// ECMP routing manager.
pub struct EcmpRouter;

impl EcmpRouter {
    /// Create a new ECMP router.
    pub fn new() -> Self {
        Self
    }

    /// Setup ECMP routing with multiple default gateways.
    ///
    /// Creates a main table with multiple default routes of equal
    /// metric, enabling the kernel's nexthop multipath routing.
    pub async fn setup_ecmp(
        &self,
        routes: &[EcmpRoute],
    ) -> Result<(), RoutingError> {
        if routes.is_empty() {
            return Err(RoutingError::RouteModify(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no routes provided for ECMP",
            )));
        }

        info!("Setting up ECMP routing with {} paths", routes.len());

        // Build the ip route command with nexthop entries
        let mut args = vec!["route".to_string(), "replace".to_string(), "default".to_string()];

        if routes.len() == 1 {
            // Single route — just add it normally
            args.push("via".to_string());
            args.push(routes[0].gateway.clone());
            args.push("dev".to_string());
            args.push(routes[0].interface.clone());
            if routes[0].metric > 0 {
                args.push("metric".to_string());
                args.push(routes[0].metric.to_string());
            }
        } else {
            // Multiple routes — use nexthop syntax
            for route in routes {
                args.push("nexthop".to_string());
                args.push("via".to_string());
                args.push(route.gateway.clone());
                args.push("dev".to_string());
                args.push(route.interface.clone());
                if route.weight > 1 {
                    args.push("weight".to_string());
                    args.push(route.weight.to_string());
                }
            }
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
                format!("ip route failed: {}", stderr),
            )));
        }

        debug!("ECMP routing configured: {}", args.join(" "));
        Ok(())
    }

    /// Remove ECMP routes.
    pub async fn teardown_ecmp(&self, routes: &[EcmpRoute]) -> Result<(), RoutingError> {
        info!("Tearing down ECMP routing");

        for route in routes {
            let output = tokio::process::Command::new("ip")
                .args([
                    "route",
                    "del",
                    "default",
                    "via",
                    &route.gateway,
                    "dev",
                    &route.interface,
                ])
                .output()
                .await
                .map_err(|e| {
                    RoutingError::RouteModify(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                })?;

            if !output.status.success() {
                debug!("Route deletion note: {}", String::from_utf8_lossy(&output.stderr));
            }
        }

        Ok(())
    }
}

/// A single ECMP route entry.
#[derive(Debug, Clone)]
pub struct EcmpRoute {
    pub gateway: String,
    pub interface: String,
    pub metric: u32,
    /// Weight for this route (1-255). Higher = more traffic.
    pub weight: u8,
}
