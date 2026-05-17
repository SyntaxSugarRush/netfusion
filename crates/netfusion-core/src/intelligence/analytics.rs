// SPDX-License-Identifier: MIT OR Apache-2.0

//! Advanced analytics — statistical analysis of interface performance.

use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use tracing::debug;

/// A single sampled health reading.
#[derive(Debug, Clone)]
pub struct HealthSample {
    pub timestamp: DateTime<Utc>,
    pub overall: f64,
    pub rtt: f64,
    pub jitter: f64,
    pub loss: f64,
    pub throughput: f64,
    pub stability: f64,
}

/// Statistical summary for a metric over a time window.
#[derive(Debug, Clone)]
pub struct MetricStats {
    pub mean: f64,
    pub stddev: f64,
    pub min: f64,
    pub max: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub sample_count: usize,
}

impl MetricStats {
    fn compute(values: &mut [f64]) -> Self {
        if values.is_empty() {
            return Self {
                mean: 0.0,
                stddev: 0.0,
                min: 0.0,
                max: 0.0,
                p50: 0.0,
                p95: 0.0,
                p99: 0.0,
                sample_count: 0,
            };
        }

        let count = values.len() as f64;
        let sum: f64 = values.iter().sum();
        let mean = sum / count;
        let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count;
        let stddev = variance.sqrt();

        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let min = values[0];
        let max = values[values.len() - 1];
        let p50 = percentile(values, 50.0);
        let p95 = percentile(values, 95.0);
        let p99 = percentile(values, 99.0);

        Self {
            mean,
            stddev,
            min,
            max,
            p50,
            p95,
            p99,
            sample_count: values.len(),
        }
    }
}

/// Compute the nth percentile from a sorted slice.
fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (pct / 100.0) * (sorted.len() as f64 - 1.0);
    let lower = idx.floor() as usize;
    let upper = idx.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let frac = idx - lower as f64;
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }
}

/// Analytics report for a single interface.
#[derive(Debug, Clone)]
pub struct InterfaceAnalytics {
    pub interface: String,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub sample_count: usize,

    /// Overall health statistics.
    pub health: MetricStats,

    /// RTT statistics (ms).
    pub rtt: MetricStats,

    /// Jitter statistics (ms).
    pub jitter: MetricStats,

    /// Packet loss statistics (%).
    pub loss: MetricStats,

    /// Throughput statistics (Mbps).
    pub throughput: MetricStats,

    /// Stability statistics.
    pub stability: MetricStats,

    /// Number of score drops below 50%.
    pub degraded_periods: usize,

    /// Number of complete outages (score < 10%).
    pub outage_periods: usize,

    /// Average duration between state changes.
    pub mean_time_between_changes: Option<f64>, // seconds
}

impl InterfaceAnalytics {
    /// Generate analytics from a series of health samples.
    pub fn from_samples(interface: String, samples: &[HealthSample]) -> Self {
        if samples.is_empty() {
            return Self {
                interface,
                window_start: Utc::now(),
                window_end: Utc::now(),
                sample_count: 0,
                health: MetricStats::compute(&mut []),
                rtt: MetricStats::compute(&mut []),
                jitter: MetricStats::compute(&mut []),
                loss: MetricStats::compute(&mut []),
                throughput: MetricStats::compute(&mut []),
                stability: MetricStats::compute(&mut []),
                degraded_periods: 0,
                outage_periods: 0,
                mean_time_between_changes: None,
            };
        }

        let mut health_vals: Vec<f64> = samples.iter().map(|s| s.overall).collect();
        let mut rtt_vals: Vec<f64> = samples.iter().map(|s| s.rtt).collect();
        let mut jitter_vals: Vec<f64> = samples.iter().map(|s| s.jitter).collect();
        let mut loss_vals: Vec<f64> = samples.iter().map(|s| s.loss).collect();
        let mut throughput_vals: Vec<f64> = samples.iter().map(|s| s.throughput).collect();
        let mut stability_vals: Vec<f64> = samples.iter().map(|s| s.stability).collect();

        let degraded_periods = samples.iter().filter(|s| s.overall < 50.0).count();
        let outage_periods = samples.iter().filter(|s| s.overall < 10.0).count();

        // Calculate mean time between state changes
        let mean_time_between_changes = if samples.len() > 1 {
            let mut intervals = Vec::new();
            for i in 1..samples.len() {
                let delta = (samples[i].timestamp - samples[i - 1].timestamp)
                    .num_seconds() as f64;
                intervals.push(delta);
            }
            if !intervals.is_empty() {
                Some(intervals.iter().sum::<f64>() / intervals.len() as f64)
            } else {
                None
            }
        } else {
            None
        };

        Self {
            interface: interface.clone(),
            window_start: samples[0].timestamp,
            window_end: samples[samples.len() - 1].timestamp,
            sample_count: samples.len(),
            health: MetricStats::compute(&mut health_vals),
            rtt: MetricStats::compute(&mut rtt_vals),
            jitter: MetricStats::compute(&mut jitter_vals),
            loss: MetricStats::compute(&mut loss_vals),
            throughput: MetricStats::compute(&mut throughput_vals),
            stability: MetricStats::compute(&mut stability_vals),
            degraded_periods,
            outage_periods,
            mean_time_between_changes,
        }
    }

    /// Get a reliability rating based on analytics.
    pub fn reliability_rating(&self) -> ReliabilityRating {
        if self.sample_count == 0 {
            return ReliabilityRating::Unknown;
        }

        let uptime_pct = 100.0
            - (self.degraded_periods as f64 / self.sample_count as f64) * 100.0;
        let outage_pct =
            (self.outage_periods as f64 / self.sample_count as f64) * 100.0;

        if uptime_pct >= 99.0 && outage_pct == 0.0 {
            ReliabilityRating::Excellent
        } else if uptime_pct >= 95.0 && outage_pct < 2.0 {
            ReliabilityRating::Good
        } else if uptime_pct >= 80.0 {
            ReliabilityRating::Fair
        } else {
            ReliabilityRating::Poor
        }
    }
}

/// Interface reliability rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliabilityRating {
    Excellent,
    Good,
    Fair,
    Poor,
    Unknown,
}

/// Aggregated performance report across all interfaces.
#[derive(Debug, Clone)]
pub struct PerformanceReport {
    pub interfaces: Vec<InterfaceAnalytics>,
    pub generated_at: DateTime<Utc>,
}

impl PerformanceReport {
    /// Find the most reliable interface.
    pub fn best_interface(&self) -> Option<&InterfaceAnalytics> {
        self.interfaces
            .iter()
            .max_by(|a, b| {
                let rating_a = a.reliability_rating();
                let rating_b = b.reliability_rating();
                // Order: Excellent > Good > Fair > Poor > Unknown
                let rank = |r: ReliabilityRating| match r {
                    ReliabilityRating::Excellent => 5,
                    ReliabilityRating::Good => 4,
                    ReliabilityRating::Fair => 3,
                    ReliabilityRating::Poor => 2,
                    ReliabilityRating::Unknown => 1,
                };
                rank(rating_a).cmp(&rank(rating_b))
            })
    }

    /// Find interfaces with poor reliability.
    pub fn problematic_interfaces(&self) -> Vec<&InterfaceAnalytics> {
        self.interfaces
            .iter()
            .filter(|i| {
                matches!(
                    i.reliability_rating(),
                    ReliabilityRating::Poor | ReliabilityRating::Fair
                )
            })
            .collect()
    }
}

/// Rolling window analytics tracker.
pub struct AnalyticsTracker {
    /// Maximum samples to retain per interface.
    max_samples: usize,

    /// Health samples per interface.
    samples: std::collections::HashMap<String, VecDeque<HealthSample>>,
}

impl AnalyticsTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            max_samples,
            samples: std::collections::HashMap::new(),
        }
    }

    /// Add a health sample for an interface.
    pub fn add_sample(&mut self, interface: &str, sample: HealthSample) {
        let queue = self.samples.entry(interface.to_string()).or_default();
        queue.push_back(sample);

        // Trim to max
        while queue.len() > self.max_samples {
            queue.pop_front();
        }
    }

    /// Generate analytics for a specific interface.
    pub fn analyze(&self, interface: &str) -> Option<InterfaceAnalytics> {
        let samples = self.samples.get(interface)?;
        let samples_vec: Vec<_> = samples.iter().cloned().collect();
        Some(InterfaceAnalytics::from_samples(
            interface.to_string(),
            &samples_vec,
        ))
    }

    /// Generate a full performance report.
    pub fn generate_report(&self) -> PerformanceReport {
        let interfaces: Vec<_> = self
            .samples
            .keys()
            .filter_map(|iface| self.analyze(iface))
            .collect();

        PerformanceReport {
            interfaces,
            generated_at: Utc::now(),
        }
    }
}

impl Default for AnalyticsTracker {
    fn default() -> Self {
        Self::new(1000) // Default: keep last 1000 samples per interface
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sample(time_offset_secs: i64, score: f64) -> HealthSample {
        HealthSample {
            timestamp: Utc::now() + chrono::Duration::seconds(time_offset_secs),
            overall: score,
            rtt: 10.0,
            jitter: 1.0,
            loss: 0.0,
            throughput: 100.0,
            stability: 90.0,
        }
    }

    #[test]
    fn test_percentile() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((percentile(&values, 50.0) - 5.5).abs() < 0.01);
        assert!((percentile(&values, 95.0) - 9.55).abs() < 0.1);
        assert!((percentile(&values, 99.0) - 9.91).abs() < 0.1);
    }

    #[test]
    fn test_metric_stats() {
        let mut values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let stats = MetricStats::compute(&mut values);
        assert_eq!(stats.mean, 30.0);
        assert_eq!(stats.min, 10.0);
        assert_eq!(stats.max, 50.0);
        assert_eq!(stats.sample_count, 5);
    }

    #[test]
    fn test_analytics_from_samples() {
        let samples = vec![
            make_sample(-60, 90.0),
            make_sample(-30, 85.0),
            make_sample(0, 88.0),
        ];
        let analytics = InterfaceAnalytics::from_samples("eth0".into(), &samples);
        assert_eq!(analytics.interface, "eth0");
        assert_eq!(analytics.sample_count, 3);
        assert!(analytics.health.mean > 80.0);
        assert_eq!(analytics.degraded_periods, 0);
        assert_eq!(analytics.outage_periods, 0);
    }

    #[test]
    fn test_reliability_rating_excellent() {
        let mut samples = Vec::new();
        for i in 0..100 {
            samples.push(make_sample(i, 95.0));
        }
        let analytics = InterfaceAnalytics::from_samples("eth0".into(), &samples);
        assert_eq!(analytics.reliability_rating(), ReliabilityRating::Excellent);
    }

    #[test]
    fn test_reliability_rating_poor() {
        let mut samples = Vec::new();
        for i in 0..100 {
            let score = if i % 2 == 0 { 20.0 } else { 90.0 };
            samples.push(make_sample(i, score));
        }
        let analytics = InterfaceAnalytics::from_samples("eth0".into(), &samples);
        assert_eq!(analytics.reliability_rating(), ReliabilityRating::Poor);
    }

    #[test]
    fn test_rolling_tracker() {
        let mut tracker = AnalyticsTracker::new(10);

        for i in 0..5 {
            tracker.add_sample("eth0", make_sample(i, 80.0 + i as f64));
        }

        let report = tracker.generate_report();
        assert_eq!(report.interfaces.len(), 1);
        assert_eq!(report.interfaces[0].sample_count, 5);
    }

    #[test]
    fn test_rolling_trim() {
        let mut tracker = AnalyticsTracker::new(5);

        for i in 0..10 {
            tracker.add_sample("eth0", make_sample(i, 50.0));
        }

        let analytics = tracker.analyze("eth0").unwrap();
        assert_eq!(analytics.sample_count, 5); // Trimmed to max
    }

    #[test]
    fn test_best_interface() {
        let mut tracker = AnalyticsTracker::new(100);

        // Good interface
        for i in 0..20 {
            tracker.add_sample("eth0", make_sample(i, 90.0));
        }
        // Bad interface
        for i in 0..20 {
            tracker.add_sample("eth1", make_sample(i, 30.0));
        }

        let report = tracker.generate_report();
        let best = report.best_interface().unwrap();
        assert_eq!(best.interface, "eth0");
    }

    #[test]
    fn test_problematic_interfaces() {
        let mut tracker = AnalyticsTracker::new(100);

        for i in 0..20 {
            tracker.add_sample("good", make_sample(i, 95.0));
            tracker.add_sample("bad", make_sample(i, 20.0));
        }

        let report = tracker.generate_report();
        let problematic = report.problematic_interfaces();
        assert_eq!(problematic.len(), 1);
        assert_eq!(problematic[0].interface, "bad");
    }
}
