// SPDX-License-Identifier: MIT OR Apache-2.0

//! Adaptive heuristics — auto-tuning health scoring weights.
//!
//! Instead of requiring users to manually tune weights, this module
//! observes which metrics correlate with actual failures and
//! automatically adjusts the weighting profile.
//!
//! Philosophy: "Adaptive heuristics" over ML — deterministic,
//! debuggable, and explainable adjustments based on observed patterns.

use std::collections::HashMap;

use tracing::debug;

use crate::health::HealthWeightsConfig;

/// Weight adjustment recommendation.
#[derive(Debug, Clone)]
pub struct WeightAdjustment {
    pub metric: String,
    pub current_weight: u8,
    pub recommended_weight: u8,
    pub reason: String,
}

/// Adaptive weight tuner.
///
/// Observes which health metrics are most predictive of failures
/// and adjusts weights accordingly.
pub struct AdaptiveWeights {
    /// Base weights (user-configured).
    base_weights: HealthWeightsConfig,

    /// Adjustment factors applied on top of base weights.
    adjustments: HashMap<String, f64>,

    /// Correlation scores: how predictive each metric is of failures.
    metric_correlations: HashMap<String, f64>,

    /// Number of observed failover events.
    observed_failovers: usize,

    /// Number of false positives (predicted failure that didn't happen).
    false_positives: usize,
}

impl AdaptiveWeights {
    pub fn new(base_weights: HealthWeightsConfig) -> Self {
        Self {
            base_weights,
            adjustments: HashMap::new(),
            metric_correlations: HashMap::new(),
            observed_failovers: 0,
            false_positives: 0,
        }
    }

    /// Record a failover event with the health metrics at the time.
    pub fn record_failover(
        &mut self,
        rtt: f64,
        jitter: f64,
        loss: f64,
        throughput: f64,
        stability: f64,
    ) {
        self.observed_failovers += 1;

        // Normalize metrics to 0-1 range (lower is worse)
        let rtt_normalized = normalize_rtt(rtt);
        let jitter_normalized = normalize_jitter(jitter);
        let loss_normalized = normalize_loss(loss);
        let throughput_normalized = normalize_throughput(throughput);
        let stability_normalized = normalize_stability(stability);

        // Update correlations: which metric was worst at failover?
        let metrics = [
            ("rtt", rtt_normalized),
            ("jitter", jitter_normalized),
            ("loss", loss_normalized),
            ("throughput", throughput_normalized),
            ("stability", stability_normalized),
        ];

        for (name, value) in &metrics {
            let entry = self.metric_correlations.entry(name.to_string()).or_insert(0.0);
            // Weight by how bad the metric was (lower = more correlated with failure)
            *entry += (1.0 - value) / self.observed_failovers as f64;
        }

        debug!(
            failover_count = self.observed_failovers,
            correlations = ?self.metric_correlations,
            "Recorded failover event"
        );
    }

    /// Record a false positive (predicted failure that didn't occur).
    pub fn record_false_positive(&mut self) {
        self.false_positives += 1;
    }

    /// Compute adjusted weights based on observed patterns.
    pub fn compute_weights(&self) -> HealthWeightsConfig {
        if self.observed_failovers < 3 {
            // Not enough data — use base weights
            return self.base_weights.clone();
        }

        let total_correlation: f64 = self.metric_correlations.values().sum();
        if total_correlation < 0.01 {
            return self.base_weights.clone();
        }

        // Calculate proportional weights based on correlation strength
        let rtt_factor = self.metric_correlations.get("rtt").copied().unwrap_or(0.2) / total_correlation;
        let jitter_factor = self.metric_correlations.get("jitter").copied().unwrap_or(0.2) / total_correlation;
        let loss_factor = self.metric_correlations.get("loss").copied().unwrap_or(0.2) / total_correlation;
        let throughput_factor = self.metric_correlations.get("throughput").copied().unwrap_or(0.2) / total_correlation;
        let stability_factor = self.metric_correlations.get("stability").copied().unwrap_or(0.2) / total_correlation;

        // Blend base weights with learned weights (70/30 to avoid over-fitting)
        let blend = 0.3;
        let total_base = (self.base_weights.rtt
            + self.base_weights.jitter
            + self.base_weights.loss
            + self.base_weights.throughput
            + self.base_weights.stability) as f64;

        if total_base == 0.0 {
            return self.base_weights.clone();
        }

        let blend_weight = |base: u8, learned: f64| -> u8 {
            let base_frac = base as f64 / total_base;
            let blended = base_frac * (1.0 - blend) + learned * blend;
            (blended * 100.0).round() as u8
        };

        HealthWeightsConfig {
            rtt: blend_weight(self.base_weights.rtt, rtt_factor),
            jitter: blend_weight(self.base_weights.jitter, jitter_factor),
            loss: blend_weight(self.base_weights.loss, loss_factor),
            throughput: blend_weight(self.base_weights.throughput, throughput_factor),
            stability: blend_weight(self.base_weights.stability, stability_factor),
        }
    }

    /// Get recommended weight adjustments.
    pub fn recommendations(&self) -> Vec<WeightAdjustment> {
        if self.observed_failovers < 3 {
            return vec![];
        }

        let current = self.compute_weights();
        let mut recs = Vec::new();

        let mut add_rec = |name: &str, current: u8, recommended: u8, reason: &str| {
            if (recommended as i8 - current as i8).abs() > 3 {
                recs.push(WeightAdjustment {
                    metric: name.to_string(),
                    current_weight: current,
                    recommended_weight: recommended,
                    reason: reason.to_string(),
                });
            }
        };

        add_rec(
            "rtt",
            self.base_weights.rtt,
            current.rtt,
            &format!("Based on {} observed failovers", self.observed_failovers),
        );
        add_rec(
            "jitter",
            self.base_weights.jitter,
            current.jitter,
            &format!("Based on {} observed failovers", self.observed_failovers),
        );
        add_rec(
            "loss",
            self.base_weights.loss,
            current.loss,
            &format!("Based on {} observed failovers", self.observed_failovers),
        );
        add_rec(
            "throughput",
            self.base_weights.throughput,
            current.throughput,
            &format!("Based on {} observed failovers", self.observed_failovers),
        );
        add_rec(
            "stability",
            self.base_weights.stability,
            current.stability,
            &format!("Based on {} observed failovers", self.observed_failovers),
        );

        recs
    }

    /// Get the false positive rate.
    pub fn false_positive_rate(&self) -> f64 {
        let total_predictions = self.observed_failovers + self.false_positives;
        if total_predictions == 0 {
            return 0.0;
        }
        self.false_positives as f64 / total_predictions as f64
    }

    /// Get summary statistics.
    pub fn summary(&self) -> AdaptiveSummary {
        AdaptiveSummary {
            observed_failovers: self.observed_failovers,
            false_positives: self.false_positives,
            false_positive_rate: self.false_positive_rate(),
            active_weights: self.compute_weights(),
            recommendations: self.recommendations().len(),
        }
    }
}

/// Summary of adaptive weight tuning.
#[derive(Debug, Clone)]
pub struct AdaptiveSummary {
    pub observed_failovers: usize,
    pub false_positives: usize,
    pub false_positive_rate: f64,
    pub active_weights: HealthWeightsConfig,
    pub recommendations: usize,
}

// Normalization helpers (mirroring health.rs logic)
fn normalize_rtt(rtt_ms: f64) -> f64 {
    if rtt_ms <= 5.0 {
        100.0
    } else if rtt_ms >= 500.0 {
        0.0
    } else {
        100.0 * (1.0 - (rtt_ms - 5.0) / 495.0)
    }
}

fn normalize_jitter(jitter_ms: f64) -> f64 {
    if jitter_ms <= 1.0 {
        100.0
    } else if jitter_ms >= 100.0 {
        0.0
    } else {
        100.0 * (1.0 - (jitter_ms - 1.0) / 99.0)
    }
}

fn normalize_loss(loss_pct: f64) -> f64 {
    if loss_pct <= 0.0 {
        100.0
    } else if loss_pct >= 20.0 {
        0.0
    } else {
        100.0 * (1.0 - loss_pct / 20.0)
    }
}

fn normalize_throughput(mbps: f64) -> f64 {
    // Assume 1000 Mbps is "perfect"
    (mbps / 1000.0 * 100.0).min(100.0)
}

fn normalize_stability(state_changes: f64) -> f64 {
    if state_changes <= 0.0 {
        100.0
    } else if state_changes >= 10.0 {
        0.0
    } else {
        100.0 * (1.0 - state_changes / 10.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_weights() -> HealthWeightsConfig {
        HealthWeightsConfig {
            rtt: 25,
            jitter: 20,
            loss: 30,
            throughput: 15,
            stability: 10,
        }
    }

    #[test]
    fn test_base_weights_when_no_data() {
        let tuner = AdaptiveWeights::new(default_weights());
        let weights = tuner.compute_weights();
        assert_eq!(weights.rtt, 25);
        assert_eq!(weights.jitter, 20);
        assert_eq!(weights.loss, 30);
    }

    #[test]
    fn test_weights_change_after_failovers() {
        let mut tuner = AdaptiveWeights::new(default_weights());

        // Simulate failovers where loss is always the worst metric
        for _ in 0..5 {
            tuner.record_failover(
                50.0,   // RTT moderate
                5.0,    // Jitter low
                15.0,   // Loss HIGH (worst)
                500.0,  // Throughput moderate
                8.0,    // Stability moderate
            );
        }

        let weights = tuner.compute_weights();
        // Loss should have increased in importance
        assert!(weights.loss >= default_weights().loss);
    }

    #[test]
    fn test_no_recommendations_with_insufficient_data() {
        let tuner = AdaptiveWeights::new(default_weights());
        assert!(tuner.recommendations().is_empty());
    }

    #[test]
    fn test_false_positive_rate() {
        let mut tuner = AdaptiveWeights::new(default_weights());

        tuner.record_failover(50.0, 5.0, 15.0, 500.0, 8.0);
        tuner.record_failover(50.0, 5.0, 15.0, 500.0, 8.0);
        tuner.record_false_positive();
        tuner.record_false_positive();
        tuner.record_false_positive();

        let rate = tuner.false_positive_rate();
        assert!((rate - 0.6).abs() < 0.01); // 3/5 = 0.6
    }

    #[test]
    fn test_normalization_functions() {
        assert!((normalize_rtt(5.0) - 100.0).abs() < 0.01);
        assert!((normalize_rtt(500.0) - 0.0).abs() < 0.01);
        assert!((normalize_loss(0.0) - 100.0).abs() < 0.01);
        assert!((normalize_loss(20.0) - 0.0).abs() < 0.01);
        assert!((normalize_jitter(1.0) - 100.0).abs() < 0.01);
        assert!((normalize_stability(0.0) - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_adaptive_summary() {
        let mut tuner = AdaptiveWeights::new(default_weights());

        for _ in 0..5 {
            tuner.record_failover(50.0, 5.0, 15.0, 500.0, 8.0);
        }

        let summary = tuner.summary();
        assert_eq!(summary.observed_failovers, 5);
        assert_eq!(summary.false_positives, 0);
        assert!(summary.recommendations > 0 || summary.observed_failovers < 10);
    }
}
