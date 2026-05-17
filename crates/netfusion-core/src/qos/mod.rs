// SPDX-License-Identifier: MIT OR Apache-2.0

//! QoS (Quality of Service) subsystem.
//!
//! Provides:
//! - fq_codel / CAKE / HTB queue discipline management via `tc`
//! - ECN (Explicit Congestion Notification) configuration
//! - DSCP tagging via nftables
//! - Per-interface QoS overrides

pub mod qdisc;
pub mod dscp;
