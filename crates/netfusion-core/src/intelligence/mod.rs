// SPDX-License-Identifier: MIT OR Apache-2.0

//! Intelligence layer for NetFusion.
//!
//! Provides:
//! - Predictive failover analysis (trend detection)
//! - Advanced analytics (statistical analysis)
//! - Adaptive heuristics (auto-tuning weights)

pub mod analytics;
pub mod predictive;
pub mod adaptive;

pub use analytics::{InterfaceAnalytics, PerformanceReport};
pub use predictive::{FailurePrediction, PredictiveEngine};
pub use adaptive::{AdaptiveWeights, WeightAdjustment};
