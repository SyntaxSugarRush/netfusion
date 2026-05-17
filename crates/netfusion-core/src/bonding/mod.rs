// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bonding and aggregation module.
//!
//! Manages bond groups, member interfaces, and failover logic.

mod manager;
mod failover;

pub use manager::BondManager;
pub use failover::FailoverEngine;
