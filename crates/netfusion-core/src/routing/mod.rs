// SPDX-License-Identifier: MIT OR Apache-2.0

//! Routing engine and policy management.
//!
//! Manages routes, policy routing rules, and safe apply with rollback.

mod engine;
mod path_steering;
mod safe_apply;
pub mod balancer;

pub use engine::RoutingEngine;
pub use path_steering::{PathSteerer, SteeringConfig, SteeringDecision, SteeringReason};
pub use safe_apply::{SafeApply, ApplyResult};
