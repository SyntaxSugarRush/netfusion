// SPDX-License-Identifier: MIT OR Apache-2.0

//! Monitoring engine for continuous health probing.
//!
//! Runs background probes on configurable intervals to collect:
//! - RTT via ICMP ping
//! - Packet loss via ping success rate
//! - Jitter via RTT variance
//! - Throughput estimation via /proc/net/dev deltas
//! - Stability via state change frequency

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::{broadcast, RwLock};
use tokio::time::interval;
use tracing::{debug, info, warn};

use netfusion_shared::config::HealthWeights;
use netfusion_shared::types::{HealthScore, InterfaceInfo};

use crate::health::{
    compute_health, ema_smooth, exceeds_hysteresis, HealthWeightsConfig,
};

/// Configuration for the monitoring engine.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Interval between health probes.
    pub probe_interval: Duration,

    /// ICMP ping target for latency testing (when interface has a gateway).
    pub ping_targets: Vec<String>,

    /// Alpha value for EMA smoothing (0.0-1.0).
    pub ema_alpha: f64,

    /// Hysteresis threshold percentage for failover consideration.
    pub hysteresis_threshold: u8,

    /// Timeout for individual ping probes.
    pub ping_timeout: Duration,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            probe_interval: Duration::from_secs(5),
            ping_targets: vec!["8.8.8.8".into(), "1.1.1.1".into()],
            ema_alpha: 0.3,
            hysteresis_threshold: 15,
            ping_timeout: Duration::from_secs(2),
        }
    }
}

/// Raw probe results for a single interface.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    /// Interface name.
    pub interface: String,

    /// Measured RTT in milliseconds.
    pub rtt_ms: f64,

    /// Measured jitter in milliseconds.
    pub jitter_ms: f64,

    /// Packet loss percentage (0-100).
    pub loss_percent: f64,

    /// Estimated throughput in Mbps.
    pub throughput_mbps: f64,

    /// Number of state changes in the monitoring window.
    pub state_changes: u32,

    /// Timestamp of this probe.
    pub timestamp: chrono::DateTime<Utc>,
}

/// Tracks throughput over time using /proc/net/dev deltas.
struct ThroughputTracker {
    /// Last recorded rx_bytes per interface.
    last_rx: HashMap<String, u64>,

    /// Last recorded tx_bytes per interface.
    last_tx: HashMap<String, u64>,

    /// Last measurement timestamp.
    last_time: Option<Instant>,
}

impl ThroughputTracker {
    fn new() -> Self {
        Self {
            last_rx: HashMap::new(),
            last_tx: HashMap::new(),
            last_time: None,
        }
    }

    /// Estimate throughput in Mbps for an interface.
    fn estimate(&mut self, name: &str, rx_bytes: u64, tx_bytes: u64) -> f64 {
        let now = Instant::now();

        if let (Some(last_time), Some(&last_rx), Some(&last_tx)) =
            (self.last_time, self.last_rx.get(name), self.last_tx.get(name))
        {
            let elapsed = now.duration_since(last_time).as_secs_f64();
            if elapsed > 0.0 {
                let rx_delta = rx_bytes.saturating_sub(last_rx) as f64;
                let tx_delta = tx_bytes.saturating_sub(last_tx) as f64;
                let total_bits = (rx_delta + tx_delta) * 8.0;
                let mbps = total_bits / elapsed / 1_000_000.0;

                self.last_rx.insert(name.to_string(), rx_bytes);
                self.last_tx.insert(name.to_string(), tx_bytes);
                self.last_time = Some(now);

                return mbps;
            }
        }

        self.last_rx.insert(name.to_string(), rx_bytes);
        self.last_tx.insert(name.to_string(), tx_bytes);
        self.last_time = Some(now);

        0.0
    }
}

/// The monitoring engine that continuously probes interface health.
pub struct HealthMonitor {
    config: MonitorConfig,
    weights: HealthWeightsConfig,
    /// Previous health scores for EMA smoothing.
    previous_scores: Arc<RwLock<HashMap<String, HealthScore>>>,
    /// Throughput tracker state.
    throughput_tracker: Arc<RwLock<ThroughputTracker>>,
    /// Event broadcast channel for health updates.
    event_tx: broadcast::Sender<HealthUpdate>,
}

/// Health update event emitted by the monitor.
#[derive(Debug, Clone)]
pub struct HealthUpdate {
    pub interface: String,
    pub previous_score: f64,
    pub new_score: HealthScore,
    pub smoothed_score: HealthScore,
    pub failover_candidate: bool,
    pub probe: ProbeResult,
}

impl HealthMonitor {
    /// Create a new health monitor.
    pub fn new(config: MonitorConfig, weights: HealthWeights) -> Self {
        let weights_config = HealthWeightsConfig {
            rtt: weights.rtt,
            jitter: weights.jitter,
            loss: weights.loss,
            throughput: weights.throughput,
            stability: weights.stability,
        };

        let (event_tx, _) = broadcast::channel(1024);

        Self {
            config,
            weights: weights_config,
            previous_scores: Arc::new(RwLock::new(HashMap::new())),
            throughput_tracker: Arc::new(RwLock::new(ThroughputTracker::new())),
            event_tx,
        }
    }

    /// Subscribe to health update events.
    pub fn subscribe(&self) -> broadcast::Receiver<HealthUpdate> {
        self.event_tx.subscribe()
    }

    /// Probe a single interface and compute its health score.
    pub async fn probe_interface(
        &self,
        interface: &InterfaceInfo,
    ) -> Option<HealthUpdate> {
        // Skip interfaces that are down
        if !matches!(interface.link_state, netfusion_shared::types::LinkState::Up) {
            return None;
        }

        let probe = self.run_probe(interface).await?;

        let raw_score = compute_health(
            probe.rtt_ms,
            probe.jitter_ms,
            probe.loss_percent,
            probe.throughput_mbps,
            probe.state_changes,
            &self.weights,
        );

        // Apply EMA smoothing
        let smoothed = {
            let mut prev_scores = self.previous_scores.write().await;
            if let Some(prev) = prev_scores.get(&interface.name) {
                let smoothed = ema_smooth(&raw_score, prev, self.config.ema_alpha);
                prev_scores.insert(interface.name.clone(), smoothed.clone());
                smoothed
            } else {
                prev_scores.insert(interface.name.clone(), raw_score.clone());
                raw_score.clone()
            }
        };

        // Check hysteresis for failover consideration
        let failover_candidate = {
            let prev_scores = self.previous_scores.read().await;
            if let Some(prev) = prev_scores.get(&interface.name) {
                exceeds_hysteresis(&smoothed, prev, self.config.hysteresis_threshold)
            } else {
                false
            }
        };

        let previous_score = smoothed.overall;

        let update = HealthUpdate {
            interface: interface.name.clone(),
            previous_score,
            new_score: raw_score,
            smoothed_score: smoothed,
            failover_candidate,
            probe,
        };

        // Broadcast the update
        let _ = self.event_tx.send(update.clone());

        Some(update)
    }

    /// Run a single probe cycle for an interface.
    async fn run_probe(&self, interface: &InterfaceInfo) -> Option<ProbeResult> {
        let name = &interface.name;

        // Determine ping target: gateway first, then configured targets
        let ping_target = interface
            .gateway
            .clone()
            .or_else(|| self.config.ping_targets.first().cloned());

        // Run ICMP ping probe
        let (rtt_ms, jitter_ms, loss_percent) = if let Some(ref target) = ping_target {
            match ping_probe(target, self.config.ping_timeout).await {
                Ok(result) => result,
                Err(e) => {
                    warn!("Ping probe failed for {}: {}", name, e);
                    // Return degraded scores on probe failure
                    (999.0, 100.0, 100.0)
                }
            }
        } else {
            // No ping target available, use conservative defaults
            debug!("No ping target for interface {}", name);
            (50.0, 10.0, 0.0)
        };

        // Estimate throughput from interface stats
        let throughput_mbps = {
            let mut tracker = self.throughput_tracker.write().await;
            tracker.estimate(
                name,
                interface.stats.rx_bytes,
                interface.stats.tx_bytes,
            )
        };

        // Track state changes (simplified — just count link state transitions)
        let state_changes = 0; // TODO: track via event history

        Some(ProbeResult {
            interface: name.clone(),
            rtt_ms,
            jitter_ms,
            loss_percent,
            throughput_mbps,
            state_changes,
            timestamp: Utc::now(),
        })
    }

    /// Run the monitoring loop continuously.
    pub async fn run(self: Arc<Self>, interfaces: Arc<RwLock<Vec<InterfaceInfo>>>) {
        info!("Health monitor starting with interval {:?}", self.config.probe_interval);

        let mut interval = interval(self.config.probe_interval);

        loop {
            interval.tick().await;

            let interfaces = interfaces.read().await;
            for iface in interfaces.iter() {
                if let Some(update) = self.probe_interface(iface).await {
                    debug!(
                        "Health update for {}: {:.1} (failover_candidate: {})",
                        update.interface, update.smoothed_score.overall, update.failover_candidate
                    );
                }
            }
        }
    }
}

/// Run an ICMP ping probe and return (rtt_ms, jitter_ms, loss_percent).
///
/// Uses the `ping` command as a fallback since raw ICMP requires CAP_NET_RAW.
async fn ping_probe(
    target: &str,
    timeout: Duration,
) -> Result<(f64, f64, f64), String> {
    // Try to use ping command with count and timeout
    let output = tokio::time::timeout(
        timeout * 3, // Overall timeout for the command
        tokio::process::Command::new("ping")
            .args([
                "-c", "4",    // 4 pings
                "-W", "2",    // 2 second timeout per ping
                "-i", "0.2",  // 0.2 second interval
                target,
            ])
            .output(),
    )
    .await
    .map_err(|_| format!("ping command timed out for {}", target))?
    .map_err(|e| format!("failed to run ping command: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ping exited with status: {:?}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_ping_output(&stdout)
}

/// Parse ping output to extract RTT statistics.
///
/// Expected format (from iputils ping):
///
/// ```text
/// rtt min/avg/max/mdev = 1.234/2.345/3.456/0.567 ms
/// ```
///
/// Also handles packet loss:
///
/// ```text
/// 4 packets transmitted, 3 received, 25% packet loss, time 3005ms
/// ```
fn parse_ping_output(output: &str) -> Result<(f64, f64, f64), String> {
    let mut loss_percent = 0.0;
    let mut avg_rtt = 0.0;
    let mut mdev = 0.0; // mdev ≈ jitter

    for line in output.lines() {
        // Parse packet loss
        if let Some(loss_str) = line.split(',').find(|part| part.contains("packet loss")) {
            if let Some(loss_val) = loss_str.trim().strip_suffix("% packet loss") {
                if let Ok(loss) = loss_val.trim().parse::<f64>() {
                    loss_percent = loss;
                }
            }
        }

        // Parse RTT statistics
        if line.contains("rtt min/avg/max/mdev") || line.contains("round-trip min/avg/max") {
            if let Some(stats) = line.split('=').nth(1) {
                let parts: Vec<&str> = stats.split('/').collect();
                if parts.len() >= 4 {
                    if let Ok(avg) = parts[1].trim().parse::<f64>() {
                        avg_rtt = avg;
                    }
                    if let Ok(mdev_val) = parts[3].trim_end_matches(" ms").parse::<f64>() {
                        mdev = mdev_val;
                    }
                }
            }
        }
    }

    if avg_rtt == 0.0 && loss_percent < 100.0 {
        return Err("could not parse RTT from ping output".into());
    }

    Ok((avg_rtt, mdev, loss_percent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ping_output() {
        let output = r#"
PING 8.8.8.8 (8.8.8.8) 56(84) bytes of data.
64 bytes from 8.8.8.8: icmp_seq=1 ttl=118 time=12.3 ms
64 bytes from 8.8.8.8: icmp_seq=2 ttl=118 time=11.8 ms
64 bytes from 8.8.8.8: icmp_seq=3 ttl=118 time=12.1 ms
64 bytes from 8.8.8.8: icmp_seq=4 ttl=118 time=12.5 ms

--- 8.8.8.8 ping statistics ---
4 packets transmitted, 4 received, 0% packet loss, time 3005ms
rtt min/avg/max/mdev = 11.800/12.175/12.500/0.250 ms
"#;
        let (rtt, jitter, loss) = parse_ping_output(output).unwrap();
        assert!((rtt - 12.175).abs() < 0.01);
        assert!((jitter - 0.250).abs() < 0.01);
        assert!((loss - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_ping_output_with_loss() {
        let output = r#"
PING 8.8.8.8 (8.8.8.8) 56(84) bytes of data.
64 bytes from 8.8.8.8: icmp_seq=1 ttl=118 time=15.2 ms
64 bytes from 8.8.8.8: icmp_seq=3 ttl=118 time=14.8 ms

--- 8.8.8.8 ping statistics ---
4 packets transmitted, 2 received, 50% packet loss, time 3001ms
rtt min/avg/max/mdev = 14.800/15.000/15.200/0.200 ms
"#;
        let (rtt, jitter, loss) = parse_ping_output(output).unwrap();
        assert!((rtt - 15.0).abs() < 0.01);
        assert!((loss - 50.0).abs() < 0.01);
    }
}
