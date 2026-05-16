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

/// Routing engine and policy management.
pub mod routing {}

/// Bonding and aggregation modes.
pub mod bonding {}

/// Monitoring and metrics collection.
pub mod monitoring {}

/// Tunnel management (WireGuard, QUIC, relay).
pub mod tunnels {}

/// Health scoring algorithm.
pub mod health {}

/// State persistence and crash recovery.
pub mod state {}

/// Error types.
pub mod error {}
