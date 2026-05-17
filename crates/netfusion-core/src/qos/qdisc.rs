// SPDX-License-Identifier: MIT OR Apache-2.0

//! Queue discipline (qdisc) management via `tc` command.
//!
//! Supports fq_codel, CAKE, HTB, prio, and pfifo_fast.

use std::process::Command;

use netfusion_shared::config::{QdiscConfig, QdiscType};
use tracing::{debug, info, warn};

use crate::error::CoreError;

/// Manages queue disciplines on network interfaces.
pub struct QdiscManager;

impl Default for QdiscManager {
    fn default() -> Self {
        Self::new()
    }
}

impl QdiscManager {
    pub fn new() -> Self {
        Self
    }

    /// Apply a qdisc to an interface.
    pub fn apply(&self, interface: &str, config: &QdiscConfig) -> Result<(), CoreError> {
        let qdisc_type = config.qdisc.unwrap_or(QdiscType::FqCodel);

        // First, remove any existing root qdisc
        self.remove(interface)?;

        match qdisc_type {
            QdiscType::FqCodel => self.apply_fq_codel(interface, config)?,
            QdiscType::Cake => self.apply_cake(interface, config)?,
            QdiscType::Htb => self.apply_htb(interface, config)?,
            QdiscType::Prio => self.apply_prio(interface)?,
            QdiscType::PfifoFast => self.apply_pfifo_fast(interface)?,
        }

        info!(interface, ?qdisc_type, "Applied qdisc to interface");
        Ok(())
    }

    /// Remove the root qdisc from an interface (reset to default).
    pub fn remove(&self, interface: &str) -> Result<(), CoreError> {
        let output = Command::new("tc")
            .args(["qdisc", "del", "dev", interface, "root"])
            .output()?;

        if !output.status.success() {
            // "No such file or directory" means there was no qdisc to delete — that's fine
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("No such file") && !stderr.contains("does not exist") {
                warn!(interface, %stderr, "Failed to remove existing qdisc (may be harmless)");
            }
        }
        Ok(())
    }

    /// Apply fq_codel qdisc.
    ///
    /// fq_codel combines Fair Queuing with Controlled Delay to combat
    /// bufferbloat while maintaining fairness across flows.
    fn apply_fq_codel(
        &self,
        interface: &str,
        config: &QdiscConfig,
    ) -> Result<(), CoreError> {
        let target_ms = config.target_ms.unwrap_or(5);
        let interval_ms = config.interval_ms.unwrap_or(100);
        let limit = config.limit.unwrap_or(10240);

        let output = Command::new("tc")
            .args([
                "qdisc",
                "add",
                "dev",
                interface,
                "root",
                "fq_codel",
                "limit",
                &limit.to_string(),
                "target",
                &format!("{}ms", target_ms),
                "interval",
                &format!("{}ms", interval_ms),
                "ecn",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to apply fq_codel on {}: {}",
                interface, stderr
            )));
        }

        debug!(interface, target_ms, interval_ms, limit, "Applied fq_codel");
        Ok(())
    }

    /// Apply CAKE qdisc.
    ///
    /// CAKE (Common Applications Kept Enhanced) is an advanced
    /// bufferbloat mitigation qdisc with built-in bandwidth estimation.
    fn apply_cake(&self, interface: &str, config: &QdiscConfig) -> Result<(), CoreError> {
        let mut args = vec![
            "qdisc", "add", "dev", interface, "root", "cake",
        ];

        // CAKE supports many options — we add the most common ones
        args.push("autorate-ingress");
        args.push("diffserv4");
        args.push("nat");

        if let Some(limit) = config.limit {
            args.push("limit");
            args.push(limit.to_string().leak());
        }

        let output = Command::new("tc").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to apply CAKE on {}: {}",
                interface, stderr
            )));
        }

        debug!(interface, "Applied CAKE");
        Ok(())
    }

    /// Apply HTB (Hierarchical Token Bucket) qdisc.
    ///
    /// HTB provides class-based rate limiting — useful for
    /// per-traffic-type bandwidth allocation.
    fn apply_htb(&self, interface: &str, _config: &QdiscConfig) -> Result<(), CoreError> {
        // Create root HTB qdisc
        let output = Command::new("tc")
            .args([
                "qdisc",
                "add",
                "dev",
                interface,
                "root",
                "handle",
                "1:",
                "htb",
                "default",
                "30",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to apply HTB root on {}: {}",
                interface, stderr
            )));
        }

        debug!(interface, "Applied HTB root qdisc");
        Ok(())
    }

    /// Apply prio qdisc (simple priority-based queuing).
    fn apply_prio(&self, interface: &str) -> Result<(), CoreError> {
        let output = Command::new("tc")
            .args([
                "qdisc",
                "add",
                "dev",
                interface,
                "root",
                "handle",
                "1:",
                "prio",
                "bands",
                "3",
                "priomap",
                "0", "0", "0", "0", "0", "0", "0", "0",
                "0", "0", "0", "0", "0", "0", "0", "0",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to apply prio on {}: {}",
                interface, stderr
            )));
        }

        debug!(interface, "Applied prio qdisc");
        Ok(())
    }

    /// Apply pfifo_fast qdisc (simple FIFO with 3 bands).
    fn apply_pfifo_fast(&self, interface: &str) -> Result<(), CoreError> {
        let output = Command::new("tc")
            .args([
                "qdisc",
                "add",
                "dev",
                interface,
                "root",
                "handle",
                "1:",
                "pfifo_fast",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to apply pfifo_fast on {}: {}",
                interface, stderr
            )));
        }

        debug!(interface, "Applied pfifo_fast qdisc");
        Ok(())
    }

    /// Query current qdisc on an interface.
    pub fn query(&self, interface: &str) -> Result<String, CoreError> {
        let output = Command::new("tc")
            .args(["qdisc", "show", "dev", interface])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Internal(format!(
                "Failed to query qdisc on {}: {}",
                interface, stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qdisc_config_defaults() {
        let config = QdiscConfig {
            qdisc: None,
            target_ms: None,
            interval_ms: None,
            limit: None,
        };
        assert!(config.qdisc.is_none());
        assert!(config.target_ms.is_none());
    }

    #[test]
    fn test_qdisc_type_serialization() {
        let types = [
            QdiscType::FqCodel,
            QdiscType::Cake,
            QdiscType::Htb,
            QdiscType::Prio,
            QdiscType::PfifoFast,
        ];
        for t in &types {
            let json = serde_json::to_string(t).unwrap();
            let _: QdiscType = serde_json::from_str(&json).unwrap();
        }
    }
}
