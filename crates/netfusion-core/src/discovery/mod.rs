// SPDX-License-Identifier: MIT OR Apache-2.0

//! Interface discovery engine.
//!
//! Discovers and enumerates all network interfaces on the system,
//! collecting comprehensive metadata for each.

mod scanner;
mod type_detection;
mod metadata;

pub use scanner::InterfaceScanner;
pub use metadata::collect_detailed_metadata;
