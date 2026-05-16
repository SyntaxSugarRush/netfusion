// SPDX-License-Identifier: MIT OR Apache-2.0

//! Core networking logic for NetFusion.
//!
//! This crate provides:
//! - Interface discovery and monitoring
//! - Routing engine and policy management
//! - Bonding and aggregation logic
//! - Health scoring and path selection
//! - Tunnel management

pub mod discovery;
pub mod error;
pub mod health;
pub mod monitoring;

/// Routing engine and policy management.
pub mod routing {}

/// Bonding and aggregation modes.
pub mod bonding {}

/// Tunnel management (WireGuard, QUIC, relay).
pub mod tunnels {}

/// State persistence and crash recovery.
pub mod state {}
