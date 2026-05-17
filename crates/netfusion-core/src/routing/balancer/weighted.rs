// SPDX-License-Identifier: MIT OR Apache-2.0

//! Weighted balancing based on interface health scores.
//!
//! Distributes traffic across interfaces proportionally to their
//! health scores, automatically adjusting as scores change.

use std::collections::HashMap;

use netfusion_shared::types::HealthScore;
use tracing::debug;

use crate::routing::balancer::ecmp::EcmpRoute;
use crate::routing::balancer::nftables::NftablesManager;

/// Weighted balancer that computes route weights from health scores.
pub struct WeightedBalancer {
    nftables: NftablesManager,
    /// Minimum health score to include an interface in balancing.
    min_health: f64,
    /// Current interface weights.
    weights: HashMap<String, u8>,
}

impl WeightedBalancer {
    /// Create a new weighted balancer.
    pub fn new(min_health: f64) -> Self {
        Self {
            nftables: NftablesManager::new(),
            min_health,
            weights: HashMap::new(),
        }
    }

    /// Compute weights from health scores.
    ///
    /// Interfaces below min_health are excluded. Remaining interfaces
    /// get weights proportional to their health scores (scaled to 1-255).
    pub fn compute_weights(&mut self, health: &HashMap<String, HealthScore>) -> HashMap<String, u8> {
        // Filter out unhealthy interfaces
        let viable: Vec<_> = health
            .iter()
            .filter(|(_, score)| score.overall >= self.min_health)
            .collect();

        if viable.is_empty() {
            self.weights.clear();
            return HashMap::new();
        }

        let total_health: f64 = viable.iter().map(|(_, s)| s.overall).sum();

        if total_health == 0.0 {
            self.weights.clear();
            return HashMap::new();
        }

        // Compute proportional weights (1-255 range)
        let mut new_weights = HashMap::new();
        for (name, score) in &viable {
            let ratio = score.overall / total_health;
            let weight = (ratio * 255.0).round().clamp(1.0, 255.0) as u8;
            new_weights.insert(name.to_string(), weight);
        }

        // Log weight changes
        for (name, weight) in &new_weights {
            let prev = self.weights.get(name);
            if prev != Some(weight) {
                debug!("Weight for {}: {} -> {}", name, prev.copied().unwrap_or(0), weight);
            }
        }

        self.weights = new_weights.clone();
        new_weights
    }

    /// Generate ECMP routes from weights and interface gateways.
    pub fn generate_routes(
        &self,
        gateways: &HashMap<String, String>,
    ) -> Vec<EcmpRoute> {
        self.weights
            .iter()
            .filter_map(|(name, weight)| {
                gateways.get(name).map(|gw| EcmpRoute {
                    gateway: gw.clone(),
                    interface: name.clone(),
                    metric: 100,
                    weight: *weight,
                })
            })
            .collect()
    }

    /// Setup nftables rules for flow-based load balancing.
    ///
    /// Uses nftables hash to distribute flows across interfaces
    /// based on computed weights.
    pub async fn setup_nftables_balancing(
        &self,
        gateways: &HashMap<String, String>,
    ) -> Result<(), std::io::Error> {
        self.nftables.setup_weighted_balancing(gateways, &self.weights).await
    }

    /// Tear down nftables balancing rules.
    pub async fn teardown_nftables_balancing(&self) -> Result<(), std::io::Error> {
        self.nftables.teardown_balancing().await
    }

    /// Get current weights.
    pub fn weights(&self) -> &HashMap<String, u8> {
        &self.weights
    }
}
