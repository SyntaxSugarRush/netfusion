// SPDX-License-Identifier: MIT OR Apache-2.0

//! Health scoring algorithm for NetFusion.
//!
//! Computes weighted composite health scores from raw network metrics,
//! applies exponential moving average smoothing, and implements
//! hysteresis to prevent flapping.

use netfusion_shared::types::HealthScore;

/// Normalizes a raw metric value to a 0-100 score based on configurable thresholds.
///
/// `ideal` is the value that maps to score 100.
/// `worst` is the value that maps to score 0.
/// Values between are linearly interpolated.
pub fn normalize_linear(value: f64, ideal: f64, worst: f64) -> f64 {
    if ideal == worst {
        return 100.0;
    }
    if ideal < worst {
        // Lower is better (e.g., RTT, jitter, loss)
        if value <= ideal {
            return 100.0;
        }
        if value >= worst {
            return 0.0;
        }
        return ((worst - value) / (worst - ideal)) * 100.0;
    } else {
        // Higher is better (e.g., throughput)
        if value >= ideal {
            return 100.0;
        }
        if value <= worst {
            return 0.0;
        }
        return ((value - worst) / (ideal - worst)) * 100.0;
    }
}

/// Normalizes RTT (ms) to a 0-100 score.
/// - <= 10ms = 100 (ideal: local/LAN)
/// - >= 500ms = 0 (worst: terrible latency)
pub fn normalize_rtt(rtt_ms: f64) -> f64 {
    normalize_linear(rtt_ms, 10.0, 500.0)
}

/// Normalizes jitter (ms) to a 0-100 score.
/// - <= 1ms = 100 (ideal: rock-solid)
/// - >= 100ms = 0 (worst: unusable)
pub fn normalize_jitter(jitter_ms: f64) -> f64 {
    normalize_linear(jitter_ms, 1.0, 100.0)
}

/// Normalizes packet loss (%) to a 0-100 score.
/// - 0% = 100 (ideal)
/// - 10% = 0 (worst: severe loss)
pub fn normalize_loss(loss_percent: f64) -> f64 {
    normalize_linear(loss_percent, 0.0, 10.0)
}

/// Normalizes throughput (Mbps) to a 0-100 score.
/// - >= 1000 Mbps = 100 (ideal: gigabit)
/// - <= 1 Mbps = 0 (worst: barely functional)
pub fn normalize_throughput(throughput_mbps: f64) -> f64 {
    normalize_linear(throughput_mbps, 1000.0, 1.0)
}

/// Normalizes stability to a 0-100 score.
/// Stability is measured as the number of state changes in a rolling window.
/// - 0 changes = 100 (perfectly stable)
/// - >= 10 changes = 0 (constantly flapping)
pub fn normalize_stability(state_changes: u32) -> f64 {
    normalize_linear(state_changes as f64, 0.0, 10.0)
}

/// Configuration for health scoring weights.
#[derive(Debug, Clone, Copy)]
pub struct HealthWeightsConfig {
    pub rtt: u8,
    pub jitter: u8,
    pub loss: u8,
    pub throughput: u8,
    pub stability: u8,
}

impl Default for HealthWeightsConfig {
    fn default() -> Self {
        Self {
            rtt: 30,
            jitter: 20,
            loss: 25,
            throughput: 15,
            stability: 10,
        }
    }
}

impl HealthWeightsConfig {
    /// Gaming profile: prioritize RTT and jitter.
    pub fn gaming() -> Self {
        Self {
            rtt: 40,
            jitter: 30,
            loss: 15,
            throughput: 5,
            stability: 10,
        }
    }

    /// Streaming profile: prioritize throughput and loss.
    pub fn streaming() -> Self {
        Self {
            rtt: 15,
            jitter: 20,
            loss: 25,
            throughput: 30,
            stability: 10,
        }
    }

    /// VoIP profile: prioritize loss and jitter.
    pub fn voip() -> Self {
        Self {
            rtt: 20,
            jitter: 30,
            loss: 30,
            throughput: 5,
            stability: 15,
        }
    }

    /// Bulk transfer profile: prioritize throughput.
    pub fn bulk_transfer() -> Self {
        Self {
            rtt: 10,
            jitter: 10,
            loss: 15,
            throughput: 45,
            stability: 20,
        }
    }
}

/// Computes a health score from raw metrics using weighted composition.
pub fn compute_health(
    rtt_ms: f64,
    jitter_ms: f64,
    loss_percent: f64,
    throughput_mbps: f64,
    state_changes: u32,
    weights: &HealthWeightsConfig,
) -> HealthScore {
    let rtt_score = normalize_rtt(rtt_ms);
    let jitter_score = normalize_jitter(jitter_ms);
    let loss_score = normalize_loss(loss_percent);
    let throughput_score = normalize_throughput(throughput_mbps);
    let stability_score = normalize_stability(state_changes);

    let _total_weight = (weights.rtt + weights.jitter + weights.loss + weights.throughput + weights.stability) as f64;

    HealthScore::compute(
        rtt_score,
        jitter_score,
        loss_score,
        throughput_score,
        stability_score,
        weights.rtt,
        weights.jitter,
        weights.loss,
        weights.throughput,
        weights.stability,
    )
}

/// Applies exponential moving average smoothing to a health score.
///
/// `alpha` controls the smoothing strength:
/// - alpha = 1.0: no smoothing (current value only)
/// - alpha = 0.3: moderate smoothing
/// - alpha = 0.1: heavy smoothing (slow to change)
pub fn ema_smooth(current: &HealthScore, previous: &HealthScore, alpha: f64) -> HealthScore {
    current.ema(previous, alpha)
}

/// Checks if a health score change exceeds the hysteresis threshold.
///
/// Returns true if the change is significant enough to warrant action
/// (e.g., failover). This prevents flapping on small score changes.
pub fn exceeds_hysteresis(current: &HealthScore, previous: &HealthScore, threshold_percent: u8) -> bool {
    current.exceeds_hysteresis(previous, threshold_percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_rtt() {
        assert!((normalize_rtt(5.0) - 100.0).abs() < 0.01);
        assert!((normalize_rtt(10.0) - 100.0).abs() < 0.01);
        // 255ms is roughly halfway between 10 and 500
        let mid = normalize_rtt(255.0);
        assert!(mid > 40.0 && mid < 60.0);
        assert!((normalize_rtt(500.0) - 0.0).abs() < 0.01);
        assert!(normalize_rtt(600.0) == 0.0);
    }

    #[test]
    fn test_normalize_jitter() {
        assert!((normalize_jitter(0.5) - 100.0).abs() < 0.01);
        assert!((normalize_jitter(1.0) - 100.0).abs() < 0.01);
        assert!(normalize_jitter(50.0) > 0.0);
        assert!((normalize_jitter(100.0) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_normalize_loss() {
        assert!((normalize_loss(0.0) - 100.0).abs() < 0.01);
        assert!(normalize_loss(5.0) > 0.0 && normalize_loss(5.0) < 100.0);
        assert!((normalize_loss(10.0) - 0.0).abs() < 0.01);
        assert!(normalize_loss(20.0) == 0.0);
    }

    #[test]
    fn test_normalize_throughput() {
        assert!((normalize_throughput(1000.0) - 100.0).abs() < 0.01);
        // 500 Mbps is roughly halfway between 1 and 1000
        let mid = normalize_throughput(500.0);
        assert!(mid > 40.0 && mid < 60.0);
        assert!((normalize_throughput(1.0) - 0.0).abs() < 0.01);
        assert!(normalize_throughput(0.5) == 0.0);
    }

    #[test]
    fn test_compute_health_perfect() {
        let score = compute_health(
            5.0,   // RTT
            0.5,   // Jitter
            0.0,   // Loss
            1000.0, // Throughput
            0,     // State changes
            &HealthWeightsConfig::default(),
        );
        assert!((score.overall - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_health_terrible() {
        let score = compute_health(
            500.0, // RTT
            100.0, // Jitter
            10.0,  // Loss
            1.0,   // Throughput
            10,    // State changes
            &HealthWeightsConfig::default(),
        );
        assert!((score.overall - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_health_gaming_weights() {
        let score_good_latency = compute_health(
            5.0,   // Good RTT
            1.0,   // Good jitter
            2.0,   // Some loss
            50.0,  // Moderate throughput
            1,     // Stable
            &HealthWeightsConfig::gaming(),
        );

        let score_bad_latency = compute_health(
            200.0, // Bad RTT
            50.0,  // Bad jitter
            2.0,   // Some loss
            500.0, // Good throughput
            1,     // Stable
            &HealthWeightsConfig::gaming(),
        );

        // Gaming weights prioritize latency, so good latency should score higher
        assert!(score_good_latency.overall > score_bad_latency.overall);
    }

    #[test]
    fn test_ema_smoothing() {
        let current = compute_health(
            5.0, 1.0, 0.0, 1000.0, 0,
            &HealthWeightsConfig::default(),
        );
        let previous = compute_health(
            200.0, 50.0, 5.0, 10.0, 5,
            &HealthWeightsConfig::default(),
        );

        let smoothed = ema_smooth(&current, &previous, 0.3);

        // Smoothed should be between current and previous
        assert!(smoothed.overall > previous.overall);
        assert!(smoothed.overall < current.overall);
    }

    #[test]
    fn test_hysteresis() {
        let score1 = compute_health(
            5.0, 1.0, 0.0, 1000.0, 0,
            &HealthWeightsConfig::default(),
        );
        let score2 = compute_health(
            10.0, 2.0, 0.5, 900.0, 0,
            &HealthWeightsConfig::default(),
        );

        // Small change should not exceed 15% hysteresis
        assert!(!exceeds_hysteresis(&score1, &score2, 15));

        let score3 = compute_health(
            200.0, 50.0, 5.0, 10.0, 5,
            &HealthWeightsConfig::default(),
        );

        // Large change should exceed 15% hysteresis
        assert!(exceeds_hysteresis(&score1, &score3, 15));
    }
}
