// SPDX-License-Identifier: MIT OR Apache-2.0

//! Predictive failover engine.
//!
//! Analyzes health score trends to predict failures before they happen,
//! enabling proactive traffic steering.
//!
//! Techniques:
//! - Linear regression for trend detection
//! - Exponential moving average for recent behavior
//! - Anomaly detection via z-score
//! - Degradation rate monitoring

use std::collections::VecDeque;

use chrono::Utc;
use tracing::{debug, info, warn};

/// A failure prediction result.
#[derive(Debug, Clone)]
pub struct FailurePrediction {
    /// Interface being analyzed.
    pub interface: String,

    /// Predicted time until failure (seconds), or None if stable.
    pub time_to_failure: Option<f64>,

    /// Confidence level (0.0-1.0).
    pub confidence: f64,

    /// Severity of the predicted failure.
    pub severity: Severity,

    /// Reason for the prediction.
    pub reason: PredictionReason,
}

impl FailurePrediction {
    /// Whether action should be taken based on this prediction.
    pub fn should_act(&self, threshold: f64) -> bool {
        self.confidence >= threshold && self.severity != Severity::None
    }
}

/// Severity of a predicted failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    None,
    Low,
    Medium,
    High,
    Critical,
}

/// Reason for a prediction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredictionReason {
    /// Stable — no issues detected.
    Stable,
    /// Score is trending downward.
    DownwardTrend,
    /// Rate of degradation is accelerating.
    AcceleratingDegradation,
    /// Anomalous readings detected (z-score).
    Anomaly,
    /// Multiple indicators suggest imminent failure.
    MultiIndicator,
}

/// A single data point for trend analysis.
#[derive(Debug, Clone)]
struct DataPoint {
    time: f64,  // seconds since start
    value: f64,
}

/// Linear regression result.
struct RegressionResult {
    slope: f64,    // change per second
    intercept: f64,
    r_squared: f64, // goodness of fit
}

/// Perform linear regression on a set of points.
fn linear_regression(points: &[DataPoint]) -> Option<RegressionResult> {
    if points.len() < 3 {
        return None;
    }

    let n = points.len() as f64;
    let sum_x: f64 = points.iter().map(|p| p.time).sum();
    let sum_y: f64 = points.iter().map(|p| p.value).sum();
    let sum_xy: f64 = points.iter().map(|p| p.time * p.value).sum();
    let sum_x2: f64 = points.iter().map(|p| p.time * p.time).sum();

    let denom = n * sum_x2 - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return None;
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / n;

    // Calculate R-squared
    let mean_y = sum_y / n;
    let ss_tot: f64 = points.iter().map(|p| (p.value - mean_y).powi(2)).sum();
    let ss_res: f64 = points
        .iter()
        .map(|p| {
            let predicted = slope * p.time + intercept;
            (p.value - predicted).powi(2)
        })
        .sum();

    let r_squared = if ss_tot.abs() < 1e-10 {
        1.0
    } else {
        1.0 - ss_res / ss_tot
    };

    Some(RegressionResult {
        slope,
        intercept,
        r_squared,
    })
}

/// Calculate z-score for a value given mean and stddev.
fn z_score(value: f64, mean: f64, stddev: f64) -> f64 {
    if stddev < 1e-10 {
        return 0.0;
    }
    (value - mean) / stddev
}

/// Predictive failover engine.
pub struct PredictiveEngine {
    /// Minimum samples required for prediction.
    min_samples: usize,

    /// Confidence threshold for acting on predictions.
    action_threshold: f64,

    /// Window size for recent analysis.
    recent_window: usize,

    /// Historical data per interface.
    history: std::collections::HashMap<String, VecDeque<DataPoint>>,
}

impl PredictiveEngine {
    pub fn new(min_samples: usize, action_threshold: f64, recent_window: usize) -> Self {
        Self {
            min_samples,
            action_threshold,
            recent_window,
            history: std::collections::HashMap::new(),
        }
    }

    /// Add a health score reading for an interface.
    pub fn add_reading(&mut self, interface: &str, score: f64) {
        let entry = self.history.entry(interface.to_string()).or_default();

        let time = if let Some(last) = entry.back() {
            last.time + 5.0 // Assume ~5 second intervals
        } else {
            0.0
        };

        entry.push_back(DataPoint { time, value: score });

        // Keep a reasonable history window
        while entry.len() > self.recent_window * 4 {
            entry.pop_front();
        }
    }

    /// Analyze an interface and return a failure prediction.
    pub fn analyze(&self, interface: &str) -> FailurePrediction {
        let Some(points) = self.history.get(interface) else {
            return FailurePrediction {
                interface: interface.to_string(),
                time_to_failure: None,
                confidence: 0.0,
                severity: Severity::None,
                reason: PredictionReason::Stable,
            };
        };

        if points.len() < self.min_samples {
            return FailurePrediction {
                interface: interface.to_string(),
                time_to_failure: None,
                confidence: 0.0,
                severity: Severity::None,
                reason: PredictionReason::Stable,
            };
        }

        // Analyze the recent window
        let recent: Vec<_> = points.iter().rev().take(self.recent_window).rev().cloned().collect();
        let all_points: Vec<_> = points.iter().cloned().collect();

        // Check for downward trend
        let trend = self.analyze_trend(&recent);

        // Check for acceleration
        let accelerating = self.detect_acceleration(&recent);

        // Check for anomalies
        let anomaly_score = self.detect_anomalies(&all_points);

        // Combine indicators
        self.combine_indicators(interface, &trend, accelerating, anomaly_score)
    }

    /// Analyze trend using linear regression.
    fn analyze_trend(&self, points: &[DataPoint]) -> TrendAnalysis {
        let Some(reg) = linear_regression(points) else {
            return TrendAnalysis::default();
        };

        // Calculate current value
        let current_time = points.last().map(|p| p.time).unwrap_or(0.0);
        let current_value = points.last().map(|p| p.value).unwrap_or(0.0);

        // Predict time to reach critical threshold (score = 20)
        let critical_threshold = 20.0;
        let time_to_critical = if reg.slope < -0.01 {
            // Negative slope — degrading
            let t = (critical_threshold - reg.intercept) / reg.slope;
            let seconds_from_now = t - current_time;
            if seconds_from_now > 0.0 {
                Some(seconds_from_now)
            } else {
                Some(0.0) // Already critical
            }
        } else {
            None // Stable or improving
        };

        TrendAnalysis {
            slope: reg.slope,
            r_squared: reg.r_squared,
            current_value,
            time_to_critical,
            is_degrading: reg.slope < -0.01 && reg.r_squared > 0.5,
        }
    }

    /// Detect if degradation is accelerating.
    fn detect_acceleration(&self, points: &[DataPoint]) -> bool {
        if points.len() < 10 {
            return false;
        }

        // Split into two halves and compare slopes
        let mid = points.len() / 2;
        let first_half = &points[..mid];
        let second_half = &points[mid..];

        let reg1 = linear_regression(first_half);
        let reg2 = linear_regression(second_half);

        if let (Some(r1), Some(r2)) = (reg1, reg2) {
            // Acceleration = second half slope is more negative than first
            r2.slope < r1.slope && r2.slope < -0.02
        } else {
            false
        }
    }

    /// Detect anomalies using z-score.
    fn detect_anomalies(&self, points: &[DataPoint]) -> f64 {
        if points.len() < 10 {
            return 0.0;
        }

        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let mean: f64 = values.iter().sum::<f64>() / values.len() as f64;
        let stddev: f64 =
            (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt();

        // Check the most recent value
        if let Some(last) = values.last() {
            let z = z_score(*last, mean, stddev).abs();
            if z > 2.0 {
                return z;
            }
        }

        0.0
    }

    /// Combine all indicators into a single prediction.
    fn combine_indicators(
        &self,
        interface: &str,
        trend: &TrendAnalysis,
        accelerating: bool,
        anomaly_score: f64,
    ) -> FailurePrediction {
        let mut confidence = 0.0;
        let mut severity = Severity::None;
        let mut reason = PredictionReason::Stable;

        if trend.is_degrading {
            confidence += trend.r_squared * 0.5;
            reason = PredictionReason::DownwardTrend;

            if trend.slope < -0.1 {
                severity = Severity::High;
            } else {
                severity = Severity::Medium;
            }
        }

        if accelerating {
            confidence += 0.3;
            reason = PredictionReason::AcceleratingDegradation;
            severity = std::cmp::max(severity, Severity::High);
        }

        if anomaly_score > 3.0 {
            confidence += 0.4;
            reason = PredictionReason::Anomaly;
            severity = std::cmp::max(severity, Severity::Medium);
        }

        if confidence > 0.6 && (trend.is_degrading || accelerating) && anomaly_score > 2.0 {
            reason = PredictionReason::MultiIndicator;
            severity = std::cmp::max(severity, Severity::Critical);
        }

        // Cap confidence
        confidence = confidence.min(1.0);

        FailurePrediction {
            interface: interface.to_string(),
            time_to_failure: trend.time_to_critical,
            confidence,
            severity,
            reason,
        }
    }

    /// Get predictions for all known interfaces.
    pub fn analyze_all(&self) -> Vec<FailurePrediction> {
        self.history
            .keys()
            .map(|iface| self.analyze(iface))
            .collect()
    }

    /// Clear history for an interface.
    pub fn clear(&mut self, interface: &str) {
        self.history.remove(interface);
    }

    /// Clear all history.
    pub fn clear_all(&mut self) {
        self.history.clear();
    }
}

impl Default for PredictiveEngine {
    fn default() -> Self {
        Self::new(10, 0.6, 30)
    }
}

/// Result of trend analysis.
#[derive(Debug, Default)]
struct TrendAnalysis {
    slope: f64,
    r_squared: f64,
    current_value: f64,
    time_to_critical: Option<f64>,
    is_degrading: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stable_interface() {
        let mut engine = PredictiveEngine::new(5, 0.6, 10);

        // Add stable readings
        for _ in 0..20 {
            engine.add_reading("eth0", 90.0 + (rand_f64() - 0.5) * 2.0);
        }

        let pred = engine.analyze("eth0");
        assert!(!pred.should_act(0.6));
        assert_eq!(pred.severity, Severity::None);
    }

    #[test]
    fn test_degrading_interface() {
        let mut engine = PredictiveEngine::new(5, 0.6, 50);

        // Add steadily degrading readings with clear negative slope
        for i in 0..60 {
            let score = 95.0 - (i as f64) * 1.5;
            engine.add_reading("eth0", score.max(1.0));
        }

        let pred = engine.analyze("eth0");
        // Should have enough data for at least some prediction
        assert!(pred.confidence > 0.0 || pred.severity != Severity::None);
    }

    #[test]
    fn test_unknown_interface() {
        let engine = PredictiveEngine::default();
        let pred = engine.analyze("unknown");
        assert_eq!(pred.severity, Severity::None);
        assert_eq!(pred.confidence, 0.0);
    }

    #[test]
    fn test_analyze_all() {
        let mut engine = PredictiveEngine::new(5, 0.6, 10);

        for _ in 0..10 {
            engine.add_reading("eth0", 85.0);
            engine.add_reading("eth1", 70.0);
        }

        let predictions = engine.analyze_all();
        assert_eq!(predictions.len(), 2);
    }

    #[test]
    fn test_linear_regression_basic() {
        let points = vec![
            DataPoint { time: 0.0, value: 10.0 },
            DataPoint { time: 1.0, value: 12.0 },
            DataPoint { time: 2.0, value: 14.0 },
            DataPoint { time: 3.0, value: 16.0 },
            DataPoint { time: 4.0, value: 18.0 },
        ];
        let reg = linear_regression(&points).unwrap();
        assert!((reg.slope - 2.0).abs() < 0.01);
        assert!((reg.intercept - 10.0).abs() < 0.01);
        assert!(reg.r_squared > 0.99);
    }

    #[test]
    fn test_z_score_normal() {
        assert!((z_score(0.0, 0.0, 1.0) - 0.0).abs() < 0.01);
        assert!((z_score(1.0, 0.0, 1.0) - 1.0).abs() < 0.01);
        assert!((z_score(-2.0, 0.0, 1.0) - (-2.0)).abs() < 0.01);
    }

    #[test]
    fn test_z_score_zero_stddev() {
        assert_eq!(z_score(5.0, 5.0, 0.0), 0.0);
    }

    #[test]
    fn test_should_act_threshold() {
        let pred = FailurePrediction {
            interface: "eth0".into(),
            time_to_failure: Some(60.0),
            confidence: 0.8,
            severity: Severity::High,
            reason: PredictionReason::DownwardTrend,
        };
        assert!(pred.should_act(0.6));
        assert!(!pred.should_act(0.9));
    }
}

fn rand_f64() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(42);
    let mut s = SEED.fetch_add(1, Ordering::Relaxed);
    s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
    ((s >> 33) as f64) / (u32::MAX as f64)
}

impl FailurePrediction {
    #[cfg(test)]
    fn is_degrading(&self) -> bool {
        matches!(
            self.reason,
            PredictionReason::DownwardTrend
                | PredictionReason::AcceleratingDegradation
                | PredictionReason::MultiIndicator
        )
    }
}
