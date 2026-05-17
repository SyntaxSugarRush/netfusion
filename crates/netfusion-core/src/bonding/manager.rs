// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bond group manager — creates, configures, and manages bond interfaces.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use rtnetlink::Handle;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use netfusion_shared::config::{BondConfig, BondMode};
use netfusion_shared::types::{BondState, HealthScore};

use crate::error::BondingError;

/// Manages bond groups and their member interfaces.
pub struct BondManager {
    handle: Handle,
    bonds: Arc<RwLock<HashMap<String, BondState>>>,
}

impl BondManager {
    /// Create a new bond manager.
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            bonds: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get a reference to the bond state map.
    pub fn bonds(&self) -> &Arc<RwLock<HashMap<String, BondState>>> {
        &self.bonds
    }

    /// Create a new bond group from configuration.
    pub async fn create_bond(&self, config: &BondConfig) -> Result<(), BondingError> {
        info!("Creating bond '{}' with mode {:?}", config.name, config.mode);

        // Determine bond interface name
        let bond_name = format!("netfusion{}", config.name.trim_start_matches("bond"));

        // For now, only active-backup is fully supported in MVP
        if !matches!(config.mode, BondMode::ActiveBackup) {
            warn!("Mode {:?} not fully supported yet, using active-backup semantics", config.mode);
        }

        // Create the bond interface via rtnetlink
        self.create_bond_interface(&bond_name, config).await?;

        let bond_state = BondState {
            name: config.name.clone(),
            mode: config.mode,
            active_members: Vec::new(),
            standby_members: config.members.clone(),
            failed_members: Vec::new(),
            health: None,
            failover_active: false,
            last_failover: None,
            bond_interface: Some(bond_name),
        };

        let mut bonds = self.bonds.write().await;
        bonds.insert(config.name.clone(), bond_state);

        info!("Bond '{}' created successfully", config.name);
        Ok(())
    }

    /// Delete a bond group and release its members.
    pub async fn delete_bond(&self, name: &str) -> Result<(), BondingError> {
        info!("Deleting bond '{}'", name);

        let mut bonds = self.bonds.write().await;
        let state = bonds
            .remove(name)
            .ok_or(BondingError::NotFound { name: name.to_string() })?;

        // Release all member interfaces
        for member in state.active_members.iter().chain(state.standby_members.iter()) {
            if let Err(e) = self.release_interface(member).await {
                warn!("Failed to release interface '{}': {}", member, e);
            }
        }

        // Delete the bond interface
        if let Some(ref bond_iface) = state.bond_interface {
            if let Err(e) = self.delete_bond_interface(bond_iface).await {
                warn!("Failed to delete bond interface '{}': {}", bond_iface, e);
            }
        }

        info!("Bond '{}' deleted successfully", name);
        Ok(())
    }

    /// Update bond membership based on health scores.
    /// Selects the healthiest interface as active, others as standby.
    pub async fn update_membership(
        &self,
        bond_name: &str,
        interface_health: &HashMap<String, HealthScore>,
    ) -> Result<Option<String>, BondingError> {
        let mut bonds = self.bonds.write().await;
        let state = bonds
            .get_mut(bond_name)
            .ok_or(BondingError::NotFound { name: bond_name.to_string() })?;

        // Score each member
        let mut scored_members: Vec<_> = state
            .standby_members
            .iter()
            .chain(state.active_members.iter())
            .filter_map(|member| {
                interface_health
                    .get(member)
                    .map(|score| (member.clone(), score.overall))
            })
            .collect();

        // Sort by score descending
        scored_members.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if scored_members.is_empty() {
            warn!("No healthy members for bond '{}'", bond_name);
            return Ok(None);
        }

        let best_member = scored_members[0].0.clone();
        let previous_active = state.active_members.first().cloned();

        // If the best member changed, trigger failover
        if previous_active.as_ref() != Some(&best_member) {
            info!(
                "Failover on '{}': {} -> {}",
                bond_name,
                previous_active.as_deref().unwrap_or("none"),
                best_member
            );

            state.active_members.clear();
            state.active_members.push(best_member.clone());

            // Move previous active back to standby
            if let Some(prev) = previous_active {
                if !state.standby_members.contains(&prev) {
                    state.standby_members.push(prev);
                }
            }

            // Remove active from standby list
            state.standby_members.retain(|m| m != &best_member);

            state.failover_active = true;
            state.last_failover = Some(Utc::now());

            // Remove failed members
            state.failed_members.retain(|m| interface_health.contains_key(m));

            Ok(Some(best_member))
        } else {
            Ok(None)
        }
    }

    /// Create a bond interface via rtnetlink.
    async fn create_bond_interface(
        &self,
        name: &str,
        config: &BondConfig,
    ) -> Result<(), BondingError> {
        // Create bond link via the link add API
        // The rtnetlink crate's bond support may be limited, so we fall back
        // to using ip command for now until proper netlink bond creation is available
        let output = tokio::process::Command::new("ip")
            .args([
                "link",
                "add",
                name,
                "type",
                "bond",
                "mode",
                &match config.mode {
                    BondMode::ActiveBackup => "1",
                    BondMode::BalanceRr => "0",
                    BondMode::BalanceXor => "2",
                    BondMode::Broadcast => "3",
                    BondMode::Lacp => "4",
                    BondMode::AdaptiveTlb => "5",
                    BondMode::AdaptiveAlb => "6",
                    _ => "1",
                },
            ])
            .output()
            .await
            .map_err(|e| BondingError::CreateFailed {
                name: name.to_string(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BondingError::CreateFailed {
                name: name.to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("ip command failed: {}", stderr),
                ),
            });
        }

        debug!("Created bond interface '{}' via ip command", name);

        // Enslave member interfaces
        for member in &config.members {
            if let Err(e) = self.enslave_interface(member, name).await {
                warn!("Failed to enslave '{}': {}", member, e);
            }
        }

        Ok(())
    }

    /// Delete a bond interface.
    async fn delete_bond_interface(&self, name: &str) -> Result<(), BondingError> {
        let output = tokio::process::Command::new("ip")
            .args(["link", "del", name])
            .output()
            .await
            .map_err(|e| BondingError::CreateFailed {
                name: name.to_string(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to delete bond interface '{}': {}", name, stderr);
        } else {
            debug!("Deleted bond interface '{}'", name);
        }
        Ok(())
    }

    /// Enslave an interface to a bond.
    async fn enslave_interface(&self, iface: &str, bond: &str) -> Result<(), BondingError> {
        debug!("Enslaving '{}' to '{}'", iface, bond);

        let output = tokio::process::Command::new("ip")
            .args(["link", "set", iface, "master", bond])
            .output()
            .await
            .map_err(|_| BondingError::EnslaveFailed {
                bond: bond.to_string(),
                iface: iface.to_string(),
            })?;

        if !output.status.success() {
            let _stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BondingError::EnslaveFailed {
                bond: bond.to_string(),
                iface: iface.to_string(),
            });
        }

        debug!("Enslaved '{}' to '{}'", iface, bond);
        Ok(())
    }

    /// Release an interface from its bond.
    async fn release_interface(&self, iface: &str) -> Result<(), BondingError> {
        debug!("Releasing '{}'", iface);

        let _ = tokio::process::Command::new("ip")
            .args(["link", "set", iface, "nomaster"])
            .output()
            .await;

        debug!("Released '{}'", iface);
        Ok(())
    }
}
