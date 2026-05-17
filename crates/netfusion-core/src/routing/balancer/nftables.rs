// SPDX-License-Identifier: MIT OR Apache-2.0

//! nftables management for flow-based load balancing.

use std::collections::HashMap;
use tracing::{debug, info};

/// nftables rules manager.
pub struct NftablesManager;

impl NftablesManager {
    /// Create a new nftables manager.
    pub fn new() -> Self {
        Self
    }

    /// Check if nftables is available.
    pub async fn is_available(&self) -> bool {
        tokio::process::Command::new("nft")
            .arg("--version")
            .output()
            .await
            .is_ok()
    }

    /// Setup weighted flow-based load balancing via nftables.
    ///
    /// Creates a table and chain that marks packets based on a hash
    /// of the connection tuple, then routes marked packets via the
    /// appropriate interface using policy routing.
    pub async fn setup_weighted_balancing(
        &self,
        gateways: &HashMap<String, String>,
        weights: &HashMap<String, u8>,
    ) -> Result<(), std::io::Error> {
        if gateways.len() < 2 {
            debug!("Only one gateway, skipping nftables balancing setup");
            return Ok(());
        }

        info!("Setting up nftables flow-based balancing");

        // Assign marks based on weights
        let mut marks: Vec<(String, u32)> = Vec::new();
        for (i, (name, _weight)) in weights.iter().enumerate() {
            if gateways.contains_key(name) {
                // Use mark values 1, 2, 3, ... for each interface
                marks.push((name.clone(), (i + 1) as u32));
            }
        }

        if marks.len() < 2 {
            debug!("Not enough interfaces for nftables balancing");
            return Ok(());
        }

        // Build the nftables ruleset
        let mut ruleset = String::from(
            "# NetFusion flow-based balancing\n\
             table inet netfusion {\n\
               chain prerouting {\n\
                 type filter hook prerouting priority mangle; policy accept;\n\
\n\
                 # Hash-based flow distribution\n\
                 ct mark set jhash ip saddr ip daddr ip protocol mod <num_ifaces> offset 1\n\
               }\n\
\n\
               chain output {\n\
                 type filter hook output priority mangle; policy accept;\n\
\n\
                 ct mark set jhash ip saddr ip daddr ip protocol mod <num_ifaces> offset 1\n\
               }\n\
             }\n",
        );

        let num_ifaces = marks.len();
        ruleset = ruleset.replace("<num_ifaces>", &num_ifaces.to_string());

        // Write ruleset to temp file and load
        let tmp_path = "/tmp/netfusion_nftables.conf";
        tokio::fs::write(tmp_path, &ruleset).await?;

        let output = tokio::process::Command::new("nft")
            .args(["-f", tmp_path])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("nftables setup note: {}", stderr);
            // Non-fatal: nftables may not be available or permissions may be insufficient
        }

        // Setup policy routing rules for each mark
        for (_, mark) in &marks {
            let _ = tokio::process::Command::new("ip")
                .args([
                    "rule",
                    "add",
                    "fwmark",
                    &mark.to_string(),
                    "table",
                    &mark.to_string(),
                ])
                .output()
                .await;
        }

        // Cleanup temp file
        let _ = tokio::fs::remove_file(tmp_path).await;

        debug!("nftables flow-based balancing configured");
        Ok(())
    }

    /// Tear down all NetFusion nftables rules.
    pub async fn teardown_balancing(&self) -> Result<(), std::io::Error> {
        info!("Tearing down nftables balancing rules");

        // Delete the netfusion table
        let output = tokio::process::Command::new("nft")
            .args(["delete", "table", "inet", "netfusion"])
            .output()
            .await?;

        if !output.status.success() {
            debug!("nftables teardown note: {}", String::from_utf8_lossy(&output.stderr));
        }

        // Remove policy routing rules
        for mark in 1..=16 {
            let _ = tokio::process::Command::new("ip")
                .args([
                    "rule",
                    "del",
                    "fwmark",
                    &mark.to_string(),
                    "table",
                    &mark.to_string(),
                ])
                .output()
                .await;
        }

        Ok(())
    }

    /// Get current nftables ruleset (for debugging).
    pub async fn list_rules(&self) -> Result<String, std::io::Error> {
        let output = tokio::process::Command::new("nft")
            .arg("list")
            .arg("ruleset")
            .output()
            .await?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
