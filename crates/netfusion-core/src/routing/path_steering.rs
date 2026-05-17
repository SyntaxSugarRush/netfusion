// SPDX-License-Identifier: MIT OR Apache-2.0

//! Dynamic path steering engine.
//!
//! Monitors health scores in real-time and automatically steers traffic
//! to the best available paths based on configurable policies.
//!
//! The steering engine sits between the health monitor and the routing
//! engine, making autonomous routing decisions when:
//! - An interface's health drops below a configurable threshold
//! - A better path becomes available (with hysteresis protection)
//! - Bufferbloat is detected on the active path

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;
use tracing::{debug, info};

use netfusion_shared::types::HealthScore;

use crate::monitoring::HealthUpdate;

/// Configuration for the path steering engine.
#[derive(Debug, Clone)]
pub struct SteeringConfig {
    /// Minimum health score to consider an interface viable (0-100).
    pub min_viable_score: f64,

    /// Minimum score delta before switching paths (prevents flapping).
    pub switch_hysteresis: f64,

    /// Minimum time between steering actions (cooldown).
    pub steering_cooldown: Duration,

    /// Score below which bufferbloat is suspected.
    pub bufferbloat_threshold: f64,

    /// Enable automatic failover.
    pub auto_failover: bool,
}

impl Default for SteeringConfig {
    fn default() -> Self {
        Self {
            min_viable_score: 30.0,
            switch_hysteresis: 15.0,
            steering_cooldown: Duration::from_secs(30),
            bufferbloat_threshold: 20.0,
            auto_failover: true,
        }
    }
}

/// Represents the current steering decision for a traffic class.
#[derive(Debug, Clone)]
pub struct SteeringDecision {
    /// Interface chosen for this traffic class.
    pub active_interface: String,

    /// Health score of the active interface.
    pub active_score: f64,

    /// Whether this was a forced failover.
    pub is_failover: bool,

    /// Reason for the decision.
    pub reason: SteeringReason,
}

/// Reasons for a steering decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteeringReason {
    /// Initial assignment — no previous state.
    Initial,
    /// Active interface health degraded below threshold.
    HealthDegraded,
    /// A better path became available.
    BetterPath,
    /// Bufferbloat detected on active path.
    Bufferbloat,
    /// Active interface went down.
    InterfaceDown,
    /// Cooldown prevented a switch — no change.
    CooldownActive,
    /// No viable interfaces available.
    NoViablePath,
}

/// Tracks the state of a traffic class's path assignment.
struct PathState {
    /// Currently active interface.
    active_interface: String,

    /// Score of the active interface at assignment time.
    active_score: f64,

    /// Time of last steering action.
    last_steering: Instant,

    /// Consecutive degraded readings on active interface.
    consecutive_degraded: u32,
}

/// Dynamic path steering engine.
///
/// Subscribes to health updates and emits steering decisions.
pub struct PathSteerer {
    config: SteeringConfig,

    /// Current path state per traffic class.
    path_states: HashMap<String, PathState>,

    /// Latest health scores per interface.
    health_scores: HashMap<String, HealthScore>,

    /// Latest steering decisions.
    decisions: HashMap<String, SteeringDecision>,

    /// Broadcast channel for steering decisions.
    decision_tx: broadcast::Sender<SteeringDecision>,
}

impl PathSteerer {
    pub fn new(config: SteeringConfig) -> Self {
        let (decision_tx, _) = broadcast::channel(256);

        Self {
            config,
            path_states: HashMap::new(),
            health_scores: HashMap::new(),
            decisions: HashMap::new(),
            decision_tx,
        }
    }

    /// Subscribe to steering decisions.
    pub fn subscribe(&self) -> broadcast::Receiver<SteeringDecision> {
        self.decision_tx.subscribe()
    }

    /// Process a health update and determine if steering is needed.
    pub fn on_health_update(&mut self, update: &HealthUpdate) -> Option<SteeringDecision> {
        let iface = &update.interface;
        let score = update.smoothed_score.overall;

        // Update the health score cache
        self.health_scores.insert(iface.clone(), update.smoothed_score.clone());

        // Check if the interface went down
        if score < self.config.min_viable_score {
            debug!(interface = %iface, score, "Interface health below viable threshold");
        }

        // Evaluate steering for each traffic class
        // For now, we have a single "default" traffic class
        self.evaluate_steering("default", iface, score)
    }

    /// Evaluate whether to steer traffic for a given class.
    fn evaluate_steering(
        &mut self,
        traffic_class: &str,
        updated_interface: &str,
        updated_score: f64,
    ) -> Option<SteeringDecision> {
        // Find the best viable interface
        let best = self.find_best_interface();

        let Some((best_iface, best_score)) = best else {
            // No viable interfaces
            if let Some(state) = self.path_states.get(traffic_class) {
                let decision = SteeringDecision {
                    active_interface: state.active_interface.clone(),
                    active_score: state.active_score,
                    is_failover: false,
                    reason: SteeringReason::NoViablePath,
                };
                self.decisions.insert(traffic_class.to_string(), decision.clone());
                let _ = self.decision_tx.send(decision);
            }
            return None;
        };

        // Check if we have an existing path state
        let Some(state) = self.path_states.get_mut(traffic_class) else {
            // Initial assignment
            let decision = self.make_initial_assignment(traffic_class, &best_iface, best_score);
            let _ = self.decision_tx.send(decision.clone());
            self.decisions.insert(traffic_class.to_string(), decision.clone());
            return Some(decision);
        };

        // Check cooldown
        let elapsed = state.last_steering.elapsed();
        if elapsed < self.config.steering_cooldown {
            debug!(
                traffic_class,
                cooldown_remaining = ?(self.config.steering_cooldown - elapsed),
                "Cooldown active — deferring steering"
            );
            return None;
        }

        let current = &state.active_interface;
        let current_score = state.active_score;

        // Check if current interface degraded
        if updated_interface == current && updated_score < self.config.min_viable_score {
            state.consecutive_degraded += 1;
            // Require 3 consecutive degraded readings before failover
            if state.consecutive_degraded >= 3 {
                if best_iface != *current {
                    return self.steer(traffic_class, &best_iface, best_score, SteeringReason::HealthDegraded);
                }
            }
            return None;
        }

        // Reset degraded counter if interface recovered
        if updated_interface == current && updated_score >= self.config.min_viable_score {
            state.consecutive_degraded = 0;
        }

        // Check if a better path is available (with hysteresis)
        if best_iface != *current
            && (best_score - current_score) > self.config.switch_hysteresis
        {
            return self.steer(traffic_class, &best_iface, best_score, SteeringReason::BetterPath);
        }

        // Check for bufferbloat on active path
        if current_score < self.config.bufferbloat_threshold
            && best_iface != *current
            && (best_score - current_score) > self.config.switch_hysteresis
        {
            return self.steer(traffic_class, &best_iface, best_score, SteeringReason::Bufferbloat);
        }

        None
    }

    /// Find the best viable interface based on current health scores.
    fn find_best_interface(&self) -> Option<(String, f64)> {
        self.health_scores
            .iter()
            .filter(|(_, score)| score.overall >= self.config.min_viable_score)
            .max_by(|a, b| a.1.overall.total_cmp(&b.1.overall))
            .map(|(iface, score)| (iface.clone(), score.overall))
    }

    /// Make initial path assignment.
    fn make_initial_assignment(
        &mut self,
        traffic_class: &str,
        interface: &str,
        score: f64,
    ) -> SteeringDecision {
        let decision = SteeringDecision {
            active_interface: interface.to_string(),
            active_score: score,
            is_failover: false,
            reason: SteeringReason::Initial,
        };

        self.path_states.insert(
            traffic_class.to_string(),
            PathState {
                active_interface: interface.to_string(),
                active_score: score,
                last_steering: Instant::now(),
                consecutive_degraded: 0,
            },
        );

        info!(
            traffic_class,
            interface,
            score,
            "Initial path assignment"
        );

        decision
    }

    /// Steer traffic to a new interface.
    fn steer(
        &mut self,
        traffic_class: &str,
        interface: &str,
        score: f64,
        reason: SteeringReason,
    ) -> Option<SteeringDecision> {
        if !self.config.auto_failover && reason != SteeringReason::Initial {
            debug!(traffic_class, "Auto failover disabled — skipping steer");
            return None;
        }

        let is_failover = matches!(
            reason,
            SteeringReason::HealthDegraded
                | SteeringReason::InterfaceDown
                | SteeringReason::Bufferbloat
        );

        let decision = SteeringDecision {
            active_interface: interface.to_string(),
            active_score: score,
            is_failover,
            reason: reason.clone(),
        };

        if let Some(state) = self.path_states.get_mut(traffic_class) {
            let old = &state.active_interface;
            info!(
                traffic_class,
                old_interface = %old,
                new_interface = %interface,
                old_score = state.active_score,
                new_score = score,
                ?reason,
                "Steering traffic to new path"
            );

            state.active_interface = interface.to_string();
            state.active_score = score;
            state.last_steering = Instant::now();
            state.consecutive_degraded = 0;
        } else {
            self.path_states.insert(
                traffic_class.to_string(),
                PathState {
                    active_interface: interface.to_string(),
                    active_score: score,
                    last_steering: Instant::now(),
                    consecutive_degraded: 0,
                },
            );
        }

        self.decisions.insert(traffic_class.to_string(), decision.clone());
        let _ = self.decision_tx.send(decision.clone());

        Some(decision)
    }

    /// Get the current steering decision for a traffic class.
    pub fn get_decision(&self, traffic_class: &str) -> Option<&SteeringDecision> {
        self.decisions.get(traffic_class)
    }

    /// Get all current steering decisions.
    pub fn all_decisions(&self) -> &HashMap<String, SteeringDecision> {
        &self.decisions
    }

    /// Reset all path state (e.g., on config change).
    pub fn reset(&mut self) {
        self.path_states.clear();
        self.decisions.clear();
        info!("Path steerer state reset");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netfusion_shared::types::HealthScore;

    fn make_health_update(interface: &str, score: f64) -> HealthUpdate {
        HealthUpdate {
            interface: interface.to_string(),
            previous_score: 0.0,
            new_score: HealthScore {
                overall: score,
                rtt: score,
                jitter: score,
                loss: score,
                throughput: score,
                stability: score,
                timestamp: chrono::Utc::now(),
                failover_candidate: false,
            },
            smoothed_score: HealthScore {
                overall: score,
                rtt: score,
                jitter: score,
                loss: score,
                throughput: score,
                stability: score,
                timestamp: chrono::Utc::now(),
                failover_candidate: false,
            },
            failover_candidate: false,
            probe: crate::monitoring::ProbeResult {
                interface: interface.to_string(),
                rtt_ms: 10.0,
                jitter_ms: 1.0,
                loss_percent: 0.0,
                throughput_mbps: 100.0,
                state_changes: 0,
                timestamp: chrono::Utc::now(),
            },
        }
    }

    #[test]
    fn test_initial_assignment() {
        let config = SteeringConfig::default();
        let mut steerer = PathSteerer::new(config);

        // Send a health update for a good interface
        let update = make_health_update("eth0", 85.0);
        let decision = steerer.on_health_update(&update);

        assert!(decision.is_some());
        let d = decision.unwrap();
        assert_eq!(d.active_interface, "eth0");
        assert_eq!(d.reason, SteeringReason::Initial);
    }

    #[test]
    fn test_failover_on_degradation() {
        let mut config = SteeringConfig::default();
        config.steering_cooldown = Duration::from_millis(0); // No cooldown for testing
        let mut steerer = PathSteerer::new(config);

        // Initial: eth0 is good
        steerer.on_health_update(&make_health_update("eth0", 85.0));

        // eth1 is also good but slightly worse
        steerer.on_health_update(&make_health_update("eth1", 70.0));

        // eth0 degrades significantly
        let update = make_health_update("eth0", 10.0);
        let decision = steerer.on_health_update(&update);

        // Should NOT steer immediately — needs 3 consecutive degraded readings
        assert!(decision.is_none());

        // Second degraded reading
        steerer.on_health_update(&make_health_update("eth0", 10.0));

        // Third degraded reading — should trigger failover
        let decision = steerer.on_health_update(&make_health_update("eth0", 10.0));
        assert!(decision.is_some());
        let d = decision.unwrap();
        assert_eq!(d.active_interface, "eth1");
        assert!(d.is_failover);
        assert_eq!(d.reason, SteeringReason::HealthDegraded);
    }

    #[test]
    fn test_hysteresis_prevents_flapping() {
        let mut config = SteeringConfig::default();
        config.steering_cooldown = Duration::from_millis(0);
        let mut steerer = PathSteerer::new(config);

        // eth0 at 70, eth1 at 75 — delta less than hysteresis (15)
        steerer.on_health_update(&make_health_update("eth0", 70.0));
        let decision = steerer.on_health_update(&make_health_update("eth1", 75.0));

        // Should NOT switch — delta (5) < hysteresis (15)
        if let Some(d) = decision {
            assert_eq!(d.active_interface, "eth0");
        }
    }

    #[test]
    fn test_no_viable_interfaces() {
        let mut config = SteeringConfig::default();
        config.steering_cooldown = Duration::from_millis(0);
        let mut steerer = PathSteerer::new(config);

        // All interfaces below threshold
        steerer.on_health_update(&make_health_update("eth0", 20.0));
        steerer.on_health_update(&make_health_update("eth1", 15.0));

        // No steering decision — no viable path
        assert!(steerer.path_states.is_empty());
    }

    #[test]
    fn test_cooldown_protection() {
        let config = SteeringConfig {
            steering_cooldown: Duration::from_secs(3600), // 1 hour cooldown
            ..Default::default()
        };
        let mut steerer = PathSteerer::new(config);

        // Initial assignment
        steerer.on_health_update(&make_health_update("eth0", 85.0));

        // eth0 crashes, eth1 is great — but cooldown prevents switch
        let update = make_health_update("eth0", 5.0);
        // Need 3 consecutive degraded readings
        steerer.on_health_update(&update);
        steerer.on_health_update(&update);
        let decision = steerer.on_health_update(&update);

        // Should NOT switch due to cooldown
        assert!(decision.is_none());
    }
}
