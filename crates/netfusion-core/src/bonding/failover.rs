// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failover engine — monitors health and triggers bond failover.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{broadcast, RwLock};
use tokio::time::interval;
use tracing::{debug, info};

use netfusion_shared::events::{FailoverEvent, NetfusionEvent};

use crate::bonding::BondManager;
use crate::monitoring::HealthUpdate;

/// Configuration for the failover engine.
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    /// Interval between failover evaluations.
    pub evaluation_interval: Duration,

    /// Minimum health score to consider an interface viable.
    pub min_viable_score: f64,

    /// Cooldown period after a failover before another can trigger.
    pub failover_cooldown: Duration,

    /// Number of consecutive bad readings before triggering failover.
    pub bad_reading_threshold: u32,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            evaluation_interval: Duration::from_secs(3),
            min_viable_score: 20.0,
            failover_cooldown: Duration::from_secs(30),
            bad_reading_threshold: 3,
        }
    }
}

/// Monitors interface health and triggers bond failover when needed.
pub struct FailoverEngine {
    config: FailoverConfig,
    bond_manager: Arc<BondManager>,
    event_tx: broadcast::Sender<NetfusionEvent>,
    /// Tracks consecutive bad readings per interface.
    bad_readings: Arc<RwLock<HashMap<String, u32>>>,
    /// Last failover timestamp per bond.
    last_failover: Arc<RwLock<HashMap<String, chrono::DateTime<Utc>>>>,
}

impl FailoverEngine {
    /// Create a new failover engine.
    pub fn new(
        config: FailoverConfig,
        bond_manager: Arc<BondManager>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1024);

        Self {
            config,
            bond_manager,
            event_tx,
            bad_readings: Arc::new(RwLock::new(HashMap::new())),
            last_failover: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe to failover events.
    pub fn subscribe(&self) -> broadcast::Receiver<NetfusionEvent> {
        self.event_tx.subscribe()
    }

    /// Process a health update and evaluate failover conditions.
    pub async fn process_health_update(&self, update: &HealthUpdate) {
        // Track bad readings
        if update.smoothed_score.overall < self.config.min_viable_score {
            let mut bad = self.bad_readings.write().await;
            let count = bad.entry(update.interface.clone()).or_insert(0);
            *count += 1;

            if *count >= self.config.bad_reading_threshold {
                debug!(
                    "Interface '{}' has {} consecutive bad readings",
                    update.interface, count
                );
            }
        } else {
            let mut bad = self.bad_readings.write().await;
            bad.remove(&update.interface);
        }

        // Evaluate all bonds
        self.evaluate_failover().await;
    }

    /// Evaluate all bonds for potential failover.
    async fn evaluate_failover(&self) {
        let bonds = self.bond_manager.bonds().read().await;

        for (bond_name, state) in bonds.iter() {
            // Check cooldown
            {
                let last = self.last_failover.read().await;
                if let Some(&last_time) = last.get(bond_name) {
                    let elapsed = Utc::now()
                        .signed_duration_since(last_time)
                        .num_seconds();
                    if elapsed < self.config.failover_cooldown.as_secs() as i64 {
                        debug!(
                            "Bond '{}' in cooldown ({}s remaining)",
                            bond_name,
                            self.config.failover_cooldown.as_secs() as i64 - elapsed
                        );
                        continue;
                    }
                }
            }

            if state.failover_active {
                info!(
                    "Bond '{}' failover: active={:?}, standby={:?}",
                    bond_name, state.active_members, state.standby_members
                );

                let event = NetfusionEvent::FailoverTriggered(FailoverEvent {
                    bond: bond_name.clone(),
                    timestamp: Utc::now(),
                    previous_active: state.standby_members.clone(),
                    new_active: state.active_members.clone(),
                    reason: "health degradation".into(),
                });

                let _ = self.event_tx.send(event);

                let mut last = self.last_failover.write().await;
                last.insert(bond_name.clone(), Utc::now());
            }
        }
    }

    /// Run the failover evaluation loop continuously.
    pub async fn run(self: Arc<Self>, mut health_rx: broadcast::Receiver<HealthUpdate>) {
        info!("Failover engine starting");

        let mut eval_interval = interval(self.config.evaluation_interval);

        loop {
            tokio::select! {
                // Process health updates as they arrive
                Ok(update) = health_rx.recv() => {
                    self.process_health_update(&update).await;
                }
                // Periodic evaluation
                _ = eval_interval.tick() => {
                    self.evaluate_failover().await;
                }
            }
        }
    }
}
