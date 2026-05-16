// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared types, IPC protocol, and configuration for NetFusion.
//!
//! This crate contains:
//! - Configuration schema and validation
//! - IPC message types for daemon ↔ TUI communication
//! - Shared data structures (Interface, Bond, HealthScore, etc.)
//! - Event type definitions

pub mod config;
pub mod events;
pub mod ipc;
pub mod types;
